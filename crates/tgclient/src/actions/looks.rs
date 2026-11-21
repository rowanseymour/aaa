//! What each button looks like and says, read from the game's data.

use std::collections::HashMap;

use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use tgdata::{GameData, Id};
use tgmap::Organization;

use crate::ui::glyphs;
use crate::{Loaded, Raising, Sim};

use super::layout::{GameHas, Place};
use super::tips::{Row, Said};
use super::{
    Aim, CANCEL_LOOK, COMMAND_ALL_HELP, COMMAND_ALL_ICON, COMMAND_ALL_NAME, DEPLOY,
    DEPLOY_TO_LOCATION, Entry, Give, Menu, ORDERS, PACK, Page, RALLY, RESEARCH, Refusal, SELL,
    Shown,
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
    cancel: Look,
    /// Where a site's Cancel goes.
    pub(crate) site: Option<Place>,
    command_all: Look,
    /// The tooltip's heading for what a recruit needs.
    pub(crate) requires: String,
    /// What a recruit the player can't afford says, given its name and
    /// cost.
    pub(crate) affording: String,
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
    pub amount: f32,
    /// Shown signed, as what's earned is.
    pub earned: bool,
    pub resource: String,
}

impl Quantity {
    pub fn number(&self) -> String {
        if self.earned && self.amount > 0.0 {
            format!("+{}", self.amount)
        } else {
            self.amount.to_string()
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
        ];
        let looks = (self.orders.iter().chain(places))
            .map(|(_, look)| look)
            .chain([&self.cancel, &self.command_all])
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
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &loaded.game;
    let glyphs = glyphs(game, &mut images, formats.0);
    let mut sources = HashMap::new();
    let mut image = |game: &GameData, path: &str| {
        let image = crate::ui::read_image(game, path, formats.0)?;
        let handle = images.add(image);
        sources.insert(handle.id(), path.to_string());
        Some(handle)
    };
    let commands_by_id: HashMap<Id, tgdata::Command> = game
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
    let no_target = title("ui_error_command_target|s", "Invalid target for %s");
    let choose_hero = title(
        "ui_error_missing_single_recruit_item",
        "Must select a hero to attach",
    );
    let quantities = |amounts: &[tgdata::Amount], earned: bool| -> Vec<Quantity> {
        amounts
            .iter()
            .map(|a| Quantity {
                glyph: glyphs.get(&a.kind).cloned(),
                amount: a.amount,
                earned,
                resource: a.resource.clone(),
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
                [(&cost, &l.cost, false), (&upkeep, &l.upkeep, false)]
                    .into_iter()
                    .filter(|(_, amounts, _)| !amounts.is_empty())
                    .map(|(label, amounts, earned)| Row {
                        label: label.clone(),
                        amounts: quantities(amounts, earned),
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
        cancel,
        site,
        command_all,
        requires,
        affording,
        research_affording,
        research_refused,
        choose_hero,
        no_target,
        has: GameHas::of(sim.0.rules()),
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
        Entry::CommandAll => &looks.command_all,
        Entry::Pack => &looks.pack.1,
        Entry::Sell => &looks.sell.1,
        Entry::Recruit(i) => &looks.menu(sim, building)[i].look,
    }
}
