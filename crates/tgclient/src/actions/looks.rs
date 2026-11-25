//! What each button looks like and says, read from the game's data.

use std::collections::HashMap;

use bevy::prelude::*;
use tgdata::{GameData, Id};
use tgmap::Organization;

use crate::ui::glyphs;
use crate::{Loaded, Raising, Sim};

use super::layout::{GameHas, Place};
use super::tips::{Row, Said};
use super::{
    AIR_ATTACK, ALL_ALLIES, Aim, CANCEL_LOOK, COMMAND_ALL_HELP, COMMAND_ALL_ICON, COMMAND_ALL_NAME,
    DEPLOY, DEPLOY_TO_LOCATION, ESCORT_OFF, ESCORT_ON, Entry, Give, Menu, ORDERS, PACK, Page,
    RALLY, RESEARCH, Refusal, SELECT_ALLY, SELL, Shown, TEAM_CANCEL, TEAM_COMMANDS,
};

/// How a button shows.
#[derive(Clone)]
pub(crate) struct Look {
    pub(crate) name: String,
    /// What it does, and why it can't be pressed when it can't.
    pub(crate) help: Option<String>,
    pub(crate) disabled: Option<String>,
    pub(crate) icon: Option<Handle<Image>>,
}

/// One of the things a building commissions.
pub(crate) struct Recruit {
    /// Its template, as the hotkeys name it.
    pub(crate) id: String,
    pub(crate) menu: Menu,
    pub(crate) look: Look,
    /// Its cost, production and upkeep, for the tooltip.
    pub(crate) economy: Vec<Row>,
    /// Whether the first of `economy` is its cost, which the recruiter's
    /// properties scale.
    pub(crate) costed: bool,
    /// What it raises; `None` for what can't be raised yet, shown dimmed.
    pub(crate) organization: Option<Organization>,
}

/// What the game says of a recruit it refuses.
#[derive(Clone, Debug)]
pub(crate) struct RecruitWords {
    /// For one the side can't afford, given its name and cost.
    pub(crate) affording: String,
    /// For one at an HQ with no place left, given the HQ's name.
    pub(crate) attach_full: String,
    /// For one at a settlement under siege.
    pub(crate) besieged: String,
}

impl RecruitWords {
    /// What's said of the recruit `name` refused for `refused`, given what
    /// it costs and the name of the HQ it would attach to; nothing where
    /// the game says nothing.
    pub(crate) fn refused(
        &self,
        refused: tgsim::Refused,
        name: &str,
        cost: impl FnOnce() -> f32,
        hq: impl FnOnce() -> String,
    ) -> Option<String> {
        match refused {
            tgsim::Refused::Unaffordable => {
                let cost = cost().ceil().to_string();
                Some(super::tips::fill(&self.affording, &[name, &cost]))
            }
            tgsim::Refused::Full => Some(super::tips::fill(&self.attach_full, &[&hq()])),
            tgsim::Refused::Besieged => Some(self.besieged.clone()),
            _ => None,
        }
    }
}

/// One of the buildings a regiment can put up, as its button shows.
pub(crate) struct Buildable {
    look: Look,
    /// Its cost and upkeep, for the tooltip.
    pub(crate) economy: Vec<Row>,
    /// Its description, under them.
    pub(crate) notes: Vec<String>,
}

/// One of the technologies a building researches, as its button shows.
pub(crate) struct Topic {
    pub(crate) look: Look,
    /// Its cost, for the tooltip.
    pub(crate) economy: Vec<Row>,
    /// What the tooltip says under its cost: what the technology does,
    /// then its description.
    pub(crate) notes: Vec<String>,
}

/// One of the requests the player can make of allies, as its button in
/// the team commands shows.
pub(crate) struct TeamCommand {
    /// E.g. `team_attack`, as the hotkeys name it.
    pub(crate) ids: Id,
    pub(crate) place: Option<Place>,
    pub(crate) look: Look,
}

/// The icons and names of the buttons, loaded with the match.
#[derive(Resource)]
pub struct Looks {
    /// Per [`ORDERS`], its place and look.
    pub(crate) orders: Vec<(Option<Place>, Look)>,
    pub(crate) deploy: (Option<Place>, Look),
    pub(crate) location: (Option<Place>, Look),
    pub(crate) research: (Option<Place>, Look),
    pub(crate) rally: (Option<Place>, Look),
    pub(crate) pack: (Option<Place>, Look),
    pub(crate) sell: (Option<Place>, Look),
    pub(crate) air_attack: (Option<Place>, Look),
    escort_on: (Option<Place>, Look),
    escort_off: (Option<Place>, Look),
    cancel: Look,
    /// Where a site's Cancel goes.
    pub(crate) site: Option<Place>,
    command_all: Look,
    /// The button that opens the team commands, the team commands in it,
    /// and its way back.
    pub(crate) team_commands: (Option<Place>, Look),
    pub(crate) team: Vec<TeamCommand>,
    pub(crate) team_cancel: Look,
    /// The ally page's buttons for all allies and for one, which say only
    /// what their tooltip does: the button for one, the ally's name.
    all_allies: Look,
    ally: Look,
    /// The tooltip's heading for what a recruit needs.
    pub(crate) requires: String,
    /// What a refused recruit says.
    pub(crate) recruit: RecruitWords,
    /// The same for research.
    pub(crate) research_affording: String,
    /// What research refused for anything else says, given its name.
    pub(crate) research_refused: String,
    /// What a technology that wants buildings says, given its name and
    /// theirs.
    pub(crate) research_requires: String,
    /// What Attach Hero says pressed with no hero chosen.
    pub(crate) choose_hero: String,
    /// What an order given only on things says given on the ground, with
    /// the order's name.
    pub(crate) no_target: String,
    /// For a place none of the selection can bombard.
    pub(crate) no_bombard: String,
    pub(crate) has: GameHas,
    /// What each of the map's building templates commissions, in its
    /// buttons' order.
    menus: HashMap<String, Vec<Recruit>>,
    /// What each building template researches, in its buttons' order.
    researches: HashMap<Id, Vec<Topic>>,
    /// How each template a regiment can build shows in its Build menu.
    buildings: HashMap<Id, Buildable>,
    /// What each template and technology is called, for saying what a
    /// commission needs.
    pub(crate) names: HashMap<Id, String>,
    /// The file each icon was read from, as its pixels are gone once it's
    /// first drawn.
    sources: HashMap<AssetId<Image>, String>,
}

impl Looks {
    /// The escort switch an airfield whose escort is `on` shows: the one
    /// that changes it.
    pub(crate) fn escort(&self, on: bool) -> &(Option<Place>, Look) {
        match on {
            true => &self.escort_off,
            false => &self.escort_on,
        }
    }

    /// The menu of what the selected building commissions.
    pub(crate) fn menu(&self, sim: &tgsim::World, building: Option<usize>) -> &[Recruit] {
        building
            .and_then(|b| self.menus.get(sim.buildings[b].template.as_str()))
            .map_or(&[], Vec::as_slice)
    }

    /// What the selected building is being aimed to deliver, if `page`
    /// is aiming it.
    pub fn delivered(
        &self,
        sim: &tgsim::World,
        building: Option<usize>,
        page: Page,
    ) -> Option<&tgmap::Organization> {
        let Page::Delivering(i) = page else {
            return None;
        };
        self.menu(sim, building).get(i)?.organization.as_ref()
    }

    /// How the building of `template` shows in the Build menu.
    pub(crate) fn buildable(&self, template: &Id) -> Option<&Buildable> {
        self.buildings.get(template)
    }

    /// The research menu of the selected building, as its research list
    /// has it.
    pub(crate) fn topics(&self, sim: &tgsim::World, building: Option<usize>) -> &[Topic] {
        building
            .and_then(|b| self.researches.get(&sim.buildings[b].template))
            .map_or(&[], Vec::as_slice)
    }
}

/// So much of a resource, shown by its glyph, or its name without one.
#[derive(Clone, PartialEq, Debug)]
pub struct Quantity {
    pub glyph: Option<Handle<Image>>,
    /// The path of our own interface's icon for the resource, drawn there
    /// in place of its glyph.
    pub icon: Option<String>,
    pub amount: f32,
    pub sign: Sign,
    pub resource: String,
}

/// How an amount's sign is shown.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Sign {
    /// Not at all, as what's paid is.
    #[default]
    Bare,
    /// As a gain, as what's earned is.
    Earned,
    /// As a loss, as what's kept up is, but for a limit taken up.
    Spent,
}

impl Quantity {
    pub fn number(&self) -> String {
        match self.sign {
            Sign::Earned if self.amount > 0.0 => format!("+{}", self.amount),
            Sign::Spent if self.amount > 0.0 => format!("-{}", self.amount),
            _ => self.amount.to_string(),
        }
    }

    /// What's written after its resource's glyph, or with the resource's
    /// name where it has none.
    pub fn label(&self) -> String {
        match &self.glyph {
            Some(_) => self.number(),
            None => format!("{} {}", self.number(), self.resource),
        }
    }
}

impl Looks {
    /// Every icon a button can show.
    pub fn icons(&self) -> impl Iterator<Item = &Handle<Image>> {
        let places = [
            &self.deploy,
            &self.location,
            &self.research,
            &self.rally,
            &self.pack,
            &self.sell,
            &self.air_attack,
            &self.escort_on,
            &self.escort_off,
        ];
        let looks = (self.orders.iter().chain(places))
            .map(|(_, look)| look)
            .chain([&self.cancel, &self.command_all])
            .chain([
                &self.team_commands.1,
                &self.team_cancel,
                &self.all_allies,
                &self.ally,
            ])
            .chain(self.team.iter().map(|t| &t.look))
            .chain(self.menus.values().flatten().map(|r| &r.look))
            .chain(self.researches.values().flatten().map(|t| &t.look))
            .chain(self.buildings.values().map(|b| &b.look));
        looks.filter_map(|look| look.icon.as_ref())
    }

    /// The file `icon` was read from.
    pub fn source(&self, icon: &Handle<Image>) -> Option<&str> {
        self.sources.get(&icon.id()).map(String::as_str)
    }

    /// The name of the order that aims `aim`.
    pub fn order(&self, aim: Aim) -> &str {
        let i = ORDERS.iter().position(|(_, g)| *g == Some(Give::Aim(aim)));
        i.map_or("", |i| self.orders[i].1.name.as_str())
    }
}

/// Loads how each command looks and what each building offers, for
/// either interface.
pub fn setup(
    mut commands: Commands,
    loaded: Res<Loaded>,
    raising: Res<Raising>,
    sim: Res<Sim>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<crate::ui::ImageCache>,
) {
    let game = &loaded.game;
    let glyphs = glyphs(game, &mut images, &mut cache);
    let icons = crate::ui::resource_icons(game);
    let mut sources = HashMap::new();
    let mut image = |game: &GameData, path: &str| {
        let handle = cache.get(game, &mut images, path)?;
        sources.insert(handle.id(), path.to_string());
        Some(handle)
    };
    let commands_by_id: HashMap<Id, tgdata::Command> = game
        .commands()
        .into_iter()
        .map(|c| (c.ids.clone(), c))
        .collect();
    let has = GameHas::of(sim.0.rules(), &commands_by_id);
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
    let air_attack = look_of(game, AIR_ATTACK);
    let escort_on = look_of(game, ESCORT_ON);
    let escort_off = look_of(game, ESCORT_OFF);
    let (site, cancel) = look_of(game, CANCEL_LOOK);
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
    let mut button = |name: &str| {
        let info = game.action_button(name).unwrap_or_default();
        let place = info.slot.map(|slot| Place { slot, high: false });
        let look = Look {
            name: info.name.unwrap_or_default(),
            help: info.tooltip,
            disabled: None,
            icon: info.icon.as_deref().and_then(|p| image(game, p)),
        };
        (place, look)
    };
    let team_commands = button(TEAM_COMMANDS);
    let (_, team_cancel) = button(TEAM_CANCEL);
    // Just the tooltip, as the key marks it.
    let mut tooltip_only = |name: &str| {
        let (_, look) = button(name);
        Look {
            name: look.help.unwrap_or_default(),
            help: None,
            ..look
        }
    };
    let all_allies = tooltip_only(ALL_ALLIES);
    let ally = tooltip_only(SELECT_ALLY);
    let team = (game.team_commands().into_iter())
        .map(|c| TeamCommand {
            place: c.slot.map(|slot| Place {
                slot,
                high: c.high_importance,
            }),
            look: Look {
                name: c.name,
                help: c.tooltip,
                disabled: c.disabled,
                icon: c.icon.as_deref().and_then(|p| image(game, p)),
            },
            ids: c.ids,
        })
        .collect();
    let affording = title(
        "recruit_error_affording|ss",
        "Insufficient funds for %s. Requires %s.",
    );
    let attach_full = title("hq_error_attach_full|s", "Cannot attach to %s. HQ is full.");
    let besieged = title(
        "ui_tooltip_cannot_recruit_while_under_siege",
        "Cannot recruit while under siege.",
    );
    let research_affording = title(
        "research_error_affording|ss",
        "Insufficient funds for %s. Requires %s.",
    );
    let research_requires = title("research_error_missing_requirements|ss", "%s required: %s");
    let research_refused = title("research_error_generic|s", "Could not research %s");
    let no_target = title("ui_error_command_target|s", "Invalid target for %s");
    let no_bombard = title("bombard_error_location", "Invalid bombard target");
    let choose_hero = title(
        "ui_error_missing_single_recruit_item",
        "Must select a hero to attach",
    );
    let kinds = &sim.0.rules().economy.resources;
    let quantities = |amounts: &[tgdata::Amount], sign: Sign| -> Vec<Quantity> {
        amounts
            .iter()
            .map(|a| {
                let taken_up = (kinds.iter())
                    .any(|r| r.id == a.kind && r.role == tgdata::ResourceRole::LimitedConsumed);
                Quantity {
                    glyph: glyphs.get(&a.kind).cloned(),
                    icon: icons.get(&a.kind).cloned(),
                    amount: a.amount,
                    sign: if taken_up { Sign::Bare } else { sign },
                    resource: a.resource.clone(),
                }
            })
            .collect()
    };
    let technologies = game.technologies();
    let resources = game.resources();
    let researches = (sim.0.rules().research_lists.iter())
        .map(|(template, list)| {
            let topics = list.iter().map(|item| {
                let id = &item.technology;
                let technology = technologies.get(id);
                let cost: Vec<tgdata::Amount> = (item.cost.nonzero())
                    .map(|(kind, amount)| tgdata::Amount {
                        kind: kind.clone(),
                        resource: (resources.iter())
                            .find(|r| &r.id == kind)
                            .map_or_else(|| kind.to_string(), |r| r.name.clone()),
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
                            amounts: quantities(&cost, Sign::Bare),
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
    // Kohan II's companies have no icon of their own.
    let banner_icons = game.banner_icons();
    // Buildings unpacked during the match need menus too: those of the
    // map's trucks, and of what any menu commissions.
    let mut menus = HashMap::new();
    let standing = sim.0.buildings.iter().map(|b| b.template.to_string());
    let mut templates: Vec<String> = standing.collect();
    templates.extend(
        sim.0
            .organizations
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
                let organization = sim.0.rules().raisable.get(id).cloned();
                let costed = listing.is_some_and(|l| !l.cost.is_empty());
                let economy = listing.map_or_else(Vec::new, |l| {
                    [
                        (&cost, &l.cost, Sign::Bare),
                        (&production, &l.production, Sign::Earned),
                        (&upkeep, &l.upkeep, Sign::Spent),
                    ]
                    .into_iter()
                    .filter(|(_, amounts, _)| !amounts.is_empty())
                    .map(|(label, amounts, sign)| Row {
                        label: label.clone(),
                        amounts: quantities(amounts, sign),
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
                        icon: (listing.and_then(|l| l.icon.as_deref()))
                            .or_else(|| banner_icons.get(id).map(String::as_str))
                            .and_then(|p| image(game, p)),
                    },
                    economy,
                    costed,
                    organization,
                }
            })
            .collect();
        templates.extend(menu.iter().filter_map(|r| {
            Some(
                r.organization
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
    let buildings = (sim.0.rules().buildable.keys())
        .map(|id| {
            let listing = listings.get(id);
            let economy = listing.map_or_else(Vec::new, |l| {
                [
                    (&cost, &l.cost, Sign::Bare),
                    (&upkeep, &l.upkeep, Sign::Spent),
                ]
                .into_iter()
                .filter(|(_, amounts, _)| !amounts.is_empty())
                .map(|(label, amounts, sign)| Row {
                    label: label.clone(),
                    amounts: quantities(amounts, sign),
                })
                .collect()
            });
            let look = Look {
                name: listing.map_or_else(|| id.to_string(), |l| l.name.clone()),
                help: None,
                disabled: None,
                icon: (listing.and_then(|l| l.icon.as_deref())).and_then(|p| image(game, p)),
            };
            let notes = listing
                .and_then(|l| l.description.clone())
                .into_iter()
                .collect();
            (
                id.clone(),
                Buildable {
                    look,
                    economy,
                    notes,
                },
            )
        })
        .collect();
    commands.insert_resource(Looks {
        orders,
        deploy,
        location,
        research,
        rally,
        pack,
        sell,
        air_attack,
        escort_on,
        escort_off,
        cancel,
        site,
        command_all,
        team_commands,
        team,
        team_cancel,
        all_allies,
        ally,
        requires,
        recruit: RecruitWords {
            affording,
            attach_full,
            besieged,
        },
        research_affording,
        research_refused,
        choose_hero,
        no_target,
        no_bombard,
        has,
        research_requires,
        menus,
        researches,
        buildings,
        names: (listings.iter().map(|(id, l)| (id.clone(), l.name.clone())))
            .chain((technologies.iter()).map(|(id, t)| (id.clone(), t.name.clone())))
            .collect(),
        sources,
    });
    commands.init_resource::<Shown>();
    commands.init_resource::<Refusal>();
    commands.init_resource::<Said>();
    commands.insert_resource(Page::Orders);
}

/// The look of the button for `entry`.
pub(crate) fn look_of<'a>(
    entry: Entry,
    sim: &tgsim::World,
    looks: &'a Looks,
    building: Option<usize>,
    builds: &[Id],
) -> &'a Look {
    match entry {
        Entry::Build(i) => (builds.get(i))
            .and_then(|t| looks.buildable(t))
            .map_or(&looks.cancel, |b| &b.look),
        Entry::CancelSite => &looks.cancel,
        Entry::Open(Menu::Build) => {
            let i = ORDERS.iter().position(|(_, g)| *g == Some(Give::Build));
            i.map_or(&looks.cancel, |i| &looks.orders[i].1)
        }
        Entry::Order(i) => &looks.orders[i].1,
        Entry::Cancel => &looks.cancel,
        Entry::Open(Menu::Deploy) => &looks.deploy.1,
        Entry::Open(Menu::Location) => &looks.location.1,
        Entry::Open(Menu::Research) => &looks.research.1,
        Entry::Research(i) => &looks.topics(sim, building)[i].look,
        Entry::Rally => &looks.rally.1,
        Entry::AirAttack => &looks.air_attack.1,
        Entry::Escort => {
            let on = building.is_some_and(|b| sim.buildings[b].escort);
            &looks.escort(on).1
        }
        Entry::CommandAll => &looks.command_all,
        Entry::TeamCommands => &looks.team_commands.1,
        Entry::Team(i) => looks.team.get(i).map_or(&looks.cancel, |t| &t.look),
        Entry::TeamCancel => &looks.team_cancel,
        Entry::AllAllies => &looks.all_allies,
        Entry::Ally(_) => &looks.ally,
        Entry::Pack => &looks.pack.1,
        Entry::Sell => &looks.sell.1,
        Entry::Recruit(i) => &looks.menu(sim, building)[i].look,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_recruit_is_told_in_the_games_words() {
        let words = RecruitWords {
            affording: "Insufficient funds for %s. Requires %s.".into(),
            attach_full: "Cannot attach to %s. HQ is full.".into(),
            besieged: "Cannot recruit while under siege.".into(),
        };
        let said = |refused| {
            words.refused(
                refused,
                "Rifle Regiment",
                || 249.5,
                || "Infantry HQ 1".into(),
            )
        };
        assert_eq!(
            said(tgsim::Refused::Unaffordable).as_deref(),
            Some("Insufficient funds for Rifle Regiment. Requires 250.")
        );
        assert_eq!(
            said(tgsim::Refused::Full).as_deref(),
            Some("Cannot attach to Infantry HQ 1. HQ is full.")
        );
        assert_eq!(
            said(tgsim::Refused::Besieged).as_deref(),
            Some("Cannot recruit while under siege.")
        );
        // Where the game says nothing.
        assert_eq!(said(tgsim::Refused::Lacking), None);
    }

    #[test]
    fn an_amount_is_signed_as_gained_or_lost() {
        let number = |amount, sign| {
            let q = Quantity {
                glyph: None,
                icon: None,
                amount,
                sign,
                resource: "oil".into(),
            };
            q.number()
        };
        assert_eq!(number(25.0, Sign::Bare), "25");
        assert_eq!(number(12.0, Sign::Earned), "+12");
        assert_eq!(number(5.0, Sign::Spent), "-5");
        assert_eq!(number(0.0, Sign::Spent), "0");
    }
}
