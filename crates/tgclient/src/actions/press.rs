//! Pressing a button, by the pointer, its hotkey or the console, and the
//! order or page it gives.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use tgdata::Id;
use tgsim::Researchable;

use crate::control_groups::ControlGroups;
use crate::intent::{Asked, Intent};
use crate::messages::{About, Failed};
use crate::{Local, Orders, Screen, Selected, SelectedBuilding, Sim};

use super::card_buttons::{AimedFor, CARD_BUTTON, CardOrder, card_order};
use super::layout::{allies, builds};
use super::looks::{Looks, Recruit, TeamCommand};
use super::tips::fill;
use super::{
    AIR_ATTACK, Aim, CANCEL_LOOK, CONSTRUCT, DEPLOY, DEPLOY_TO_LOCATION, ESCORT_OFF, ESCORT_ON,
    Entry, Give, Menu, ORDERS, PACK, Page, RALLY, RESEARCH, Refusal, SELECT_ALLY, SELL, SLOTS,
    Shown, TEAM_CANCEL, TEAM_CHAT, TEAM_COMMANDS, Target, WIDGET,
};

/// What the game's hotkeys call what a button does, and the button
/// itself by its slot: the [`Intent`]s that press it.
pub(crate) fn bound(
    entry: Entry,
    slot: usize,
    page: Page,
    (menu, builds, team): (&[Recruit], &[Id], &[TeamCommand]),
    researches: &[tgmap::Research],
) -> Vec<String> {
    let command = |ids: &str| format!("ActorCommand {ids}");
    let mut names = match entry {
        Entry::Order(i) => vec![command(ORDERS[i].0)],
        Entry::Cancel if matches!(page, Page::Menu(_)) => vec!["CancelRecruit".into()],
        Entry::Cancel => vec!["ActorCommandCancel".into()],
        Entry::Open(Menu::Deploy) => vec![command(DEPLOY)],
        Entry::Open(Menu::Location) => vec![command(DEPLOY_TO_LOCATION)],
        Entry::Open(Menu::Research) => vec![command(RESEARCH)],
        Entry::Open(Menu::Build) => vec![command(CONSTRUCT)],
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
        Entry::AirAttack => vec![command(AIR_ATTACK)],
        // Either switch, whichever shows.
        Entry::Escort => vec![command(ESCORT_ON), command(ESCORT_OFF)],
        Entry::CommandAll => vec!["CommandAll".into()],
        Entry::Build(i) => {
            let template = builds.get(i).map_or("", Id::as_str);
            vec![format!("ActorDataSelectCommand {template}")]
        }
        Entry::CancelSite => vec![command(CANCEL_LOOK)],
        Entry::TeamCommands => vec![TEAM_COMMANDS.into()],
        Entry::AllAllies => vec![SELECT_ALLY.into()],
        // Keys reach only the first nine.
        Entry::Ally(n) => vec![format!("{SELECT_ALLY} {}", n + 1)],
        Entry::Team(i) => {
            let ids = team.get(i).map_or("", |t| t.ids.as_str());
            vec![format!("TeamCommand {ids}")]
        }
        Entry::TeamCancel => vec![TEAM_CANCEL.into()],
    };
    names.push(widget(slot));
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
    groups: ResMut<'w, ControlGroups>,
    page: ResMut<'w, Page>,
    aimed_for: ResMut<'w, AimedFor>,
    time: Res<'w, Time>,
    sounds: ResMut<'w, crate::sound::Sounds>,
    refusal: ResMut<'w, Refusal>,
    chat: ResMut<'w, crate::chat::Chat>,
    failed: MessageWriter<'w, Failed>,
    /// For the names of what's refused.
    log: Option<Res<'w, crate::messages::MessageLog>>,
    words: Res<'w, crate::confirm::Words>,
    questions: Query<'w, 's, (), With<crate::confirm::Question>>,
    commands: Commands<'w, 's>,
}

impl Presser<'_, '_> {
    /// Presses the button under the card of the organization with `id`:
    /// aims Run for it alone, or has it alone rout.
    fn press_card(&mut self, id: &str) {
        let Some(player) = self.local.0 else {
            return;
        };
        let sim = &self.sim.0;
        let Some(r) = (sim.organizations.iter()).position(|o| o.id.to_string() == id) else {
            return;
        };
        match card_order(sim, Some(player), r) {
            Some(CardOrder::Run) => {
                *self.page = Page::Aiming(Aim::Run);
                self.aimed_for.0 = Some(r);
            }
            Some(CardOrder::Rout) => {
                let rout = tgsim::Command::Rout { organization: r };
                if self.orders.give(sim, player, rout).is_ok() {
                    crate::commanding::acknowledge(sim, &mut self.sounds, &self.time, player, &[r]);
                }
            }
            None => {}
        }
    }

    /// Presses the button in `slot`: gives its order, aims it, opens or
    /// works the commission menu, or cancels.
    fn press(&mut self, slot: usize) {
        let (Some(entry), Some(player)) = (self.shown.entries[slot], self.local.0) else {
            return;
        };
        let sim = &self.sim.0;
        match entry {
            Entry::Order(i) => match ORDERS[i].1 {
                Some(Give::Aim(aim)) => {
                    *self.page = Page::Aiming(aim);
                    self.aimed_for.0 = None;
                }
                Some(Give::Stop) => {
                    let selected = &self.selected.0;
                    crate::commanding::acknowledge(
                        sim,
                        &mut self.sounds,
                        &self.time,
                        player,
                        selected,
                    );
                    if self.groups.grouping(selected) {
                        self.orders.gather(player);
                    }
                    for &organization in selected {
                        let stop = tgsim::Command::Stop { organization };
                        let _ = self.orders.give(sim, player, stop);
                    }
                    *self.page = Page::Orders;
                }
                Some(give @ (Give::Rout | Give::Attach | Give::Detach | Give::DetachHero)) => {
                    let mut given = false;
                    let mut short = Vec::new();
                    for &organization in &self.selected.0 {
                        if let Some(command) = give.command(organization) {
                            let check = sim.check(player, &command);
                            if give == Give::Attach && check == Err(tgsim::Refused::Unaffordable) {
                                short.push(organization);
                            }
                            given |= self.orders.give(sim, player, command).is_ok();
                        }
                    }
                    // An order refused outright gets no reply.
                    if given {
                        let selected = &self.selected.0;
                        crate::commanding::acknowledge(
                            sim,
                            &mut self.sounds,
                            &self.time,
                            player,
                            selected,
                        );
                    }
                    for r in short {
                        self.refuse_attach(r);
                    }
                }
                // A side with no heroes is refused without a word.
                Some(Give::AttachHero) if sim.heroes(player).is_empty() => {}
                // Heroes are chosen from their own menu, so a press here
                // alone only says one is wanted.
                Some(Give::AttachHero) => {
                    let about = self.selected.0.first().map(|&r| About::Organization(r));
                    let text = self.looks.choose_hero.clone();
                    self.failed.write(Failed {
                        text,
                        about,
                        at: None,
                    });
                }
                Some(Give::Disband) => {
                    if let &[organization] = &self.selected.0[..] {
                        let disband = tgsim::Command::Disband { organization };
                        self.ask(slot, player, disband);
                    }
                }
                Some(Give::Build) => *self.page = Page::Menu(Menu::Build),
                Some(Give::GroupBehavior) => self.groups.toggle(&self.selected.0),
                None => {}
            },
            Entry::Build(i) => *self.page = Page::Placing(i),
            Entry::CancelSite => {
                if let Some(building) = self.chosen.0 {
                    let cancel = tgsim::Command::CancelConstruction { building };
                    if let Err(e) = self.orders.give(sim, player, cancel) {
                        debug!("can't cancel {}: {e:?}", sim.buildings[building].template);
                    }
                }
            }
            Entry::Cancel | Entry::TeamCancel => *self.page = Page::Orders,
            Entry::TeamCommands => *self.page = Page::Allies,
            Entry::AllAllies => *self.page = Page::Team(Target::All),
            Entry::Ally(n) => {
                if let Some(&side) = allies(sim, Some(player)).get(n) {
                    *self.page = Page::Team(Target::Side(side));
                }
            }
            Entry::Team(i) => {
                let Page::Team(target) = *self.page else {
                    return;
                };
                // Chat is all the sim lets be asked; the rest stay up.
                if self
                    .looks
                    .team
                    .get(i)
                    .is_some_and(|t| t.ids.as_str() == TEAM_CHAT)
                {
                    let filter = match target {
                        Target::All => crate::chat::Filter::Allies,
                        Target::Side(side) => crate::chat::Filter::To(side),
                    };
                    self.chat.ask(filter);
                    *self.page = Page::Orders;
                }
            }
            Entry::Open(menu) => *self.page = Page::Menu(menu),
            Entry::Rally => *self.page = Page::Rally,
            Entry::AirAttack => *self.page = Page::AirAttack,
            Entry::Escort => {
                if let Some(b) = self.chosen.0 {
                    let on = !sim.buildings[b].escort;
                    let escort = tgsim::Command::Escort { building: b, on };
                    if let Err(e) = self.orders.give(sim, player, escort) {
                        debug!("can't set {}'s escort: {e:?}", sim.buildings[b].template);
                    }
                }
            }
            Entry::CommandAll => {
                let building = self.chosen.0.filter(|_| self.selected.0.is_empty());
                // One thing alone: an HQ, or a regiment attached to one.
                let one = match &self.selected.0[..] {
                    &[r] => sim.organizations[r].hq,
                    _ => None,
                };
                if let Some(hq) = shown_hq(sim, &self.selected.0, building).or(one) {
                    // As the game does, even where that leaves nothing
                    // selected.
                    self.selected.0 = sim.attached(hq).collect();
                    self.chosen.0 = None;
                }
            }
            Entry::Sell => {
                if let Some(b) = self.chosen.0 {
                    self.ask(slot, player, sell_command(b));
                }
            }
            Entry::Pack => {
                if let Some(b) = self.chosen.0
                    && let Err(e) =
                        (self.orders).give(sim, player, tgsim::Command::Pack { building: b })
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
                if recruit.organization.is_some() {
                    let template = tgmap::Id::from(recruit.id.as_str());
                    let settlement = sim.settlement_of(b);
                    let command = match settlement {
                        Some(settlement) => tgsim::Command::Recruit {
                            settlement,
                            template: template.clone(),
                            hero: None,
                        },
                        None => tgsim::Command::Commission {
                            building: b,
                            template: template.clone(),
                        },
                    };
                    // What's flown in is aimed first, and ordered where
                    // it's aimed.
                    // A settlement's companies come out of it.
                    let flown =
                        settlement.is_none() && sim.rules().airlifts.contains_key(&template);
                    let given = match flown {
                        true => sim.check(player, &command),
                        false => self.orders.give(sim, player, command),
                    };
                    if let Err(e) = given {
                        debug!("can't commission {template}: {e:?}");
                        // As the game does, it says nothing for what's not
                        // offered.
                        if e != tgsim::Refused::Lacking {
                            self.refusal.0 = Some((slot, e));
                            self.refuse_recruit(b, i, e);
                        }
                    }
                    // A plain recruit leaves the menu open for the next.
                    if flown && given.is_ok() {
                        *self.page = Page::Delivering(i);
                    }
                }
            }
            Entry::Research(i) => {
                let Some(b) = self.chosen.0 else {
                    return;
                };
                let Some(item) = sim.research_list(b).get(i) else {
                    return;
                };
                let command = tgsim::Command::Research {
                    building: b,
                    technology: item.technology.clone(),
                };
                let given = self.orders.give(sim, player, command);
                if let Err(e) = given {
                    debug!("can't research {}: {e:?}", item.technology);
                    self.refuse_research(b, i, e);
                }
            }
        }
    }
}

impl Presser<'_, '_> {
    /// Asks the player whether they're sure, then gives `command` if it
    /// can still be given. What can't be given now is refused at once, and
    /// a press while a question's up is ignored.
    fn ask(&mut self, slot: usize, player: u32, command: tgsim::Command) {
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

    /// Tells the player the selected building at `building` won't recruit
    /// the `i`th item of its menu, where the game says why.
    fn refuse_recruit(&mut self, building: usize, i: usize, refused: tgsim::Refused) {
        let log = self.log.as_deref();
        let Some(text) = recruit_refused(&self.sim.0, &self.looks, log, building, i, refused)
        else {
            return;
        };
        self.failed.write(Failed {
            text,
            about: Some(About::Building(building)),
            at: None,
        });
    }

    /// Tells the player their side can't afford to attach the organization
    /// at `r`, pointing at the HQ it would go to.
    fn refuse_attach(&mut self, r: usize) {
        let sim = &self.sim.0;
        let name = match self.log.as_deref() {
            Some(log) => log.look.organization(sim, r),
            None => sim.organizations[r].template.to_string(),
        };
        let cost = sim.attach_cost(r).ceil().to_string();
        let text = fill(&self.looks.recruit.attach_affording, &[&name, &cost]);
        let hq = sim.attaches_to(r);
        let building = (0..sim.buildings.len())
            .find(|&b| hq.is_some() && !sim.buildings[b].gone() && sim.hq_of(b) == hq);
        let packed = (0..sim.organizations.len()).find(|&o| hq.is_some() && sim.packed_hq(o) == hq);
        let about = building
            .map(About::Building)
            .or(packed.map(About::Organization));
        self.failed.write(Failed {
            text,
            about,
            at: None,
        });
    }

    /// Tells the player the selected building at `building` won't research
    /// the `i`th item it lists, and why.
    fn refuse_research(&mut self, building: usize, i: usize, refused: tgsim::Refused) {
        let sim = &self.sim.0;
        let Some(item) = sim.research_list(building).get(i) else {
            return;
        };
        let looks = &self.looks;
        let name = (looks.topics(sim, Some(building)).get(i))
            .map_or(item.technology.as_str(), |t| t.look.name.as_str());
        let text = match refused {
            tgsim::Refused::Unaffordable => {
                let cost = sim.research_cost(item).ceil().to_string();
                fill(&looks.research_affording, &[name, &cost])
            }
            _ => fill(&looks.research_refused, &[name]),
        };
        let about = About::Building(building);
        self.failed.write(Failed {
            text,
            about: Some(about),
            at: None,
        });
    }
}

/// The original's widget for the button in `slot`, which either
/// interface's button there asks for.
pub fn widget(slot: usize) -> String {
    format!("{WIDGET}_{slot}")
}

/// Presses the button an intent asks for, by what it does or by its
/// widget, if it's showing.
pub fn act(mut intents: MessageReader<Intent>, mut presser: Presser) {
    let asked = Asked::read(&mut intents);
    if asked.is_empty() {
        return;
    }
    let cards = (asked.all()).filter_map(|i| i.0.strip_prefix(CARD_BUTTON)?.strip_prefix(' '));
    for id in cards {
        presser.press_card(id);
    }
    let wanted = |name: &str| asked.wants(name);
    let building = presser.chosen.0.filter(|_| presser.selected.0.is_empty());
    let menu = presser.looks.menu(&presser.sim.0, building);
    let list = building.map_or(&[][..], |b| presser.sim.0.research_list(b));
    let builds = builds(&presser.sim.0, &presser.selected.0);
    let team = &presser.looks.team;
    let page = *presser.page;
    let slot = (0..SLOTS).find(|&slot| {
        let names = presser.shown.entries[slot]
            .filter(|e| !matches!(e, Entry::Recruit(i) if *i >= menu.len()))
            .filter(|e| !matches!(e, Entry::Research(i) if *i >= list.len()))
            .map_or(Vec::new(), |e| {
                bound(e, slot, page, (menu, &builds, team), list)
            });
        names.iter().any(|name| wanted(name))
    });
    if let Some(slot) = slot {
        presser.press(slot);
        return;
    }
    // What's done or under way, its slot left empty, is refused.
    let (Page::Menu(Menu::Research), Some(b), Some(player)) = (page, building, presser.local.0)
    else {
        return;
    };
    let sim = &presser.sim.0;
    let taken = (sim.research_list(b).iter().enumerate()).find(|(_, item)| {
        wanted(&format!("PropertyDataSelectCommand {}", item.technology))
            && sim.researchable(player, b, item) == Researchable::Taken
    });
    if let Some((i, _)) = taken {
        presser.refuse_research(b, i, tgsim::Refused::Unable);
    }
}

/// What the game says of the `i`th recruit of `building`'s menu refused
/// for `refused`, where it says anything; `log` names the HQ it would
/// attach to.
pub(super) fn recruit_refused(
    sim: &tgsim::World,
    looks: &Looks,
    log: Option<&crate::messages::MessageLog>,
    building: usize,
    i: usize,
    refused: tgsim::Refused,
) -> Option<String> {
    let recruit = looks.menu(sim, Some(building)).get(i)?;
    let cost = || {
        recruit
            .organization
            .as_ref()
            .map_or(0.0, |r| sim.cost(building, r))
    };
    let hq = || {
        let id = sim.hq_of(building);
        let hq = (sim.hqs().into_iter()).find(|h| Some(h.id) == id);
        match (log, hq) {
            (Some(log), Some(hq)) => log.look.hq(sim, &hq),
            (Some(log), None) => log.look.building(sim, building),
            (None, _) => sim.buildings[building].template.to_string(),
        }
    };
    looks.recruit.refused(refused, &recruit.look.name, cost, hq)
}

/// The HQ the selection begins with, by its id, which shows Command All:
/// the selected building, or the first selected organization, packed.
pub(crate) fn first_hq(
    sim: &tgsim::World,
    selected: &[usize],
    building: Option<usize>,
) -> Option<u32> {
    match (selected, building) {
        ([], Some(b)) => sim.hq_of(b),
        ([r, ..], _) => sim.packed_hq(*r),
        _ => None,
    }
}

/// The HQ the selection is, by its id: the selected building, or the one
/// selected organization, packed.
pub(crate) fn shown_hq(
    sim: &tgsim::World,
    selected: &[usize],
    building: Option<usize>,
) -> Option<u32> {
    match (selected, building) {
        ([], Some(b)) => sim.hq_of(b),
        ([r], _) => sim.packed_hq(*r),
        _ => None,
    }
}

/// The order to sell the building at `building`.
pub(super) fn sell_command(building: usize) -> tgsim::Command {
    tgsim::Command::Sell { building }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_go_by_the_names_the_hotkeys_bind() {
        let attach = ORDERS.iter().position(|(i, _)| *i == "hq_attach");
        let attach = Entry::Order(attach.unwrap());
        let names = |entry, page| bound(entry, 4, page, (&[], &[], &[]), &[]);
        assert_eq!(
            names(attach, Page::Orders),
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
}
