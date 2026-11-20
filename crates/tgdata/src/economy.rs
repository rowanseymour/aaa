//! What sides earn and spend: the resources, each template's own cost,
//! production and upkeep, and the settings that bound them.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::ops::{Add, AddAssign, Sub, SubAssign};
use std::sync::Arc;

use crate::strings::{glyph_image, localized};
use crate::{GameData, Id, Strings, defs, not_null, scoped};

/// The interface layout that places the resource bar's cells.
const BAR_LAYOUT: &str = "UI/Game/template_game.tgi";
/// The resource bar's template in [`BAR_LAYOUT`].
const BAR: &str = "EconomyBarWidget";

pub use defs::ResourceRole;

/// One of the resources the data defines, which costs, upkeeps and
/// production name by its id.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceKind {
    pub id: Id,
    pub role: ResourceRole,
    /// For a limited resource, the one that provides room for it.
    pub limited_by: Option<Id>,
    /// What making up a shortfall of one unit of it costs, in stockpiled
    /// resources.
    pub purchase_cost: Amounts,
    /// What the game calls it, its id where the data names nothing.
    pub name: String,
    /// What the game says of it, where it says anything.
    pub description: Option<String>,
    /// The glyph it's drawn with, by the glyph's id.
    pub icon: Option<String>,
    /// What the font's image of that glyph is called, e.g. `MONEY`.
    pub glyph: Option<String>,
}

impl ResourceKind {
    fn new(id: &str, role: ResourceRole) -> Self {
        ResourceKind {
            id: id.into(),
            role,
            limited_by: None,
            purchase_cost: Amounts::default(),
            name: id.to_string(),
            description: None,
            icon: None,
            glyph: None,
        }
    }

    /// A resource that's stockpiled and bought with.
    pub fn stockpiled(id: &str) -> Self {
        Self::new(id, ResourceRole::Stockpiled)
    }

    /// A resource that's kept up as it's produced, a shortfall of it bought
    /// at `purchase_cost`.
    pub fn upkeep(id: &str, purchase_cost: Amounts) -> Self {
        ResourceKind {
            purchase_cost,
            ..Self::new(id, ResourceRole::Upkeep)
        }
    }

    /// A resource whose use is capped by what's produced of `by`.
    pub fn limited(id: &str, by: &str) -> Self {
        ResourceKind {
            limited_by: Some(by.into()),
            ..Self::new(id, ResourceRole::LimitedConsumed)
        }
    }
}

/// So much of each resource, by id; none of a resource it doesn't name.
#[derive(Debug, Clone, Default)]
pub struct Amounts(Vec<(Id, f32)>);

impl Amounts {
    /// How much there is of a resource.
    pub fn get(&self, kind: &str) -> f32 {
        (self.0.iter())
            .find(|(id, _)| id.as_str().eq_ignore_ascii_case(kind))
            .map_or(0.0, |&(_, a)| a)
    }

    pub fn set(&mut self, kind: &str, amount: f32) {
        match (self.0.iter_mut()).find(|(id, _)| id.as_str().eq_ignore_ascii_case(kind)) {
            Some((_, a)) => *a = amount,
            None => self.0.push((kind.into(), amount)),
        }
    }

    fn credit(&mut self, kind: &Id, amount: f32) {
        match self.0.iter_mut().find(|(id, _)| id == kind) {
            Some((_, a)) => *a += amount,
            None => self.0.push((kind.clone(), amount)),
        }
    }

    /// Each resource named, in the order first named.
    pub fn iter(&self) -> impl Iterator<Item = (&Id, f32)> + '_ {
        self.0.iter().map(|(id, a)| (id, *a))
    }

    /// Each resource there's any of, in the order first named.
    pub fn nonzero(&self) -> impl Iterator<Item = (&Id, f32)> + '_ {
        self.iter().filter(|&(_, a)| a != 0.0)
    }
}

/// Equal when they have as much of every resource, a resource one doesn't
/// name being none.
impl PartialEq for Amounts {
    fn eq(&self, other: &Self) -> bool {
        let covers = |a: &Self, b: &Self| a.iter().all(|(id, n)| b.get(id.as_str()) == n);
        covers(self, other) && covers(other, self)
    }
}

impl From<&defs::Resources> for Amounts {
    fn from(r: &defs::Resources) -> Self {
        let mut out = Amounts::default();
        // The lists every cost starts from give each resource a 0.
        for (kind, amount) in r.amounts().filter(|&(_, a)| a != 0.0) {
            out.credit(kind, amount);
        }
        out
    }
}

impl AddAssign<&Amounts> for Amounts {
    fn add_assign(&mut self, other: &Amounts) {
        for (kind, amount) in other.iter() {
            self.credit(kind, amount);
        }
    }
}

impl SubAssign<&Amounts> for Amounts {
    fn sub_assign(&mut self, other: &Amounts) {
        for (kind, amount) in other.iter() {
            self.credit(kind, -amount);
        }
    }
}

impl AddAssign for Amounts {
    fn add_assign(&mut self, other: Amounts) {
        *self += &other;
    }
}

impl SubAssign for Amounts {
    fn sub_assign(&mut self, other: Amounts) {
        *self -= &other;
    }
}

impl Add<&Amounts> for Amounts {
    type Output = Amounts;
    fn add(mut self, other: &Amounts) -> Amounts {
        self += other;
        self
    }
}

impl Sub<&Amounts> for Amounts {
    type Output = Amounts;
    fn sub(mut self, other: &Amounts) -> Amounts {
        self -= other;
        self
    }
}

impl Add for Amounts {
    type Output = Amounts;
    fn add(self, other: Amounts) -> Amounts {
        self + &other
    }
}

impl Sub for Amounts {
    type Output = Amounts;
    fn sub(self, other: Amounts) -> Amounts {
        self - &other
    }
}

/// A template's own economy, as its data gives it, not counting its
/// members'.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Economy {
    /// Taken once, when it's commissioned.
    pub cost: Amounts,
    /// Earned, and kept up, per minute.
    pub production: Amounts,
    pub upkeep: Amounts,
    /// Whether it earns nothing while out of supply.
    pub supply_required: bool,
}

/// How a template supplies its side's regiments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Supply {
    /// How far around it its zone reaches.
    pub range: f32,
    /// Whether it supplies only while supplied itself, as a depot does,
    /// rather than of itself, as a Corps HQ does.
    pub relay: bool,
    /// Seconds a relay keeps going once cut off, when full.
    pub stockpile_max: f32,
}

/// How an organization template stands with supply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Logistics {
    /// Whether it's in or out of supply at all, as an HQ's isn't.
    pub supply_used: bool,
    /// Whether it can be in supply without being attached to an HQ.
    pub detached: bool,
    /// The share of their health its members lose a second out of supply
    /// while their side can't pay its upkeep.
    pub shortfall_damage: f32,
    /// For an HQ, how many regiments it takes.
    pub attachments: Option<u32>,
}

/// The settings that bound every side's economy.
#[derive(Debug, Clone, PartialEq)]
pub struct EconomyRules {
    /// The resources, in the order the data defines them.
    pub resources: Vec<ResourceKind>,
    /// The most a limiting resource provides, in total.
    pub limited_max: f32,
    /// The most a stockpile holds.
    pub stockpile_max: f32,
    /// What a stockpile starts at for sides that take the default, on maps
    /// that don't set it.
    pub stockpile_default: f32,
    /// The share of its own template's cost a building sells for.
    pub sell_factor: f32,
    /// Whether a building under siege can be sold.
    pub sell_under_siege: bool,
}

impl Default for EconomyRules {
    fn default() -> Self {
        EconomyRules {
            resources: Vec::new(),
            limited_max: f32::INFINITY,
            stockpile_max: f32::INFINITY,
            stockpile_default: 0.0,
            sell_factor: 0.0,
            sell_under_siege: true,
        }
    }
}

impl EconomyRules {
    pub fn resource(&self, id: &str) -> Option<&ResourceKind> {
        (self.resources.iter()).find(|r| r.id.as_str().eq_ignore_ascii_case(id))
    }

    /// The resources that are stockpiled, which things are bought with.
    pub fn stockpiled(&self) -> impl Iterator<Item = &Id> + '_ {
        (self.resources.iter())
            .filter(|r| r.role == ResourceRole::Stockpiled)
            .map(|r| &r.id)
    }

    /// Each limited resource with the one that provides room for it.
    pub fn limits(&self) -> impl Iterator<Item = (&Id, &Id)> + '_ {
        (self.resources.iter())
            .filter(|r| r.role == ResourceRole::LimitedConsumed)
            .filter_map(|r| Some((&r.id, r.limited_by.as_ref()?)))
    }
}

impl GameData {
    /// Each template's own [`Economy`], for those that have one, by
    /// template id. A resource the data doesn't define is an error in the
    /// data, logged once and left out.
    pub fn economies(&self) -> HashMap<Id, Economy> {
        let resources = self.resources();
        let undefined = std::sync::Mutex::new(HashSet::new());
        let of = |r: Option<&defs::Resources>| {
            let mut out = Amounts::default();
            for (id, amount) in r.into_iter().flat_map(defs::Resources::amounts) {
                if resources.iter().any(|k| &k.id == id) {
                    out.credit(id, amount);
                } else if crate::locked(&undefined).insert(id.clone()) {
                    log::warn!("{id} is named as a resource, which no [Resource] defines");
                }
            }
            out
        };
        self.by_template(|t| {
            let e = t.economy_component.as_ref()?;
            Some(Economy {
                cost: of(e.cost.as_ref()),
                production: of(e.production.as_ref()),
                upkeep: of(e.upkeep.as_ref()),
                supply_required: e.supply_required.unwrap_or(false),
            })
        })
    }

    /// The templates whose things their owner can sell.
    pub fn sellable(&self) -> HashSet<Id> {
        let sellable = self.by_template(|t| {
            let c = t.construction_component.as_ref()?;
            c.can_sell.unwrap_or(true).then_some(())
        });
        sellable.into_keys().collect()
    }

    /// Each template's [`Supply`], for those that give it, by template id.
    pub fn supplies(&self) -> HashMap<Id, Supply> {
        self.by_template(|t| {
            let s = t.supply_component.as_ref()?;
            Some(Supply {
                range: s.supply_range?,
                relay: s.is_relay.unwrap_or(false),
                stockpile_max: s.stockpile_max.unwrap_or(0.0),
            })
        })
    }

    /// Each organization template's [`Logistics`], by template id.
    pub fn logistics(&self) -> HashMap<Id, Logistics> {
        self.by_template(|t| {
            let o = t.organization_component.as_ref()?;
            Some(Logistics {
                supply_used: o.supply_used.unwrap_or(true),
                detached: o.in_supply_detached.unwrap_or(true),
                shortfall_damage: o.shortfall_damage_rate.unwrap_or(0.01),
                attachments: t
                    .hq_component
                    .as_ref()
                    .map(|h| h.max_attachments.unwrap_or(0)),
            })
        })
    }

    /// The share of its health each template gets back a second in
    /// supply, for those that get any, by template id.
    pub fn resupply_rates(&self) -> HashMap<Id, f32> {
        self.by_template(|t| {
            t.element_component
                .as_ref()?
                .resupply_rate
                .filter(|&r| r > 0.0)
        })
    }

    /// The resources the data defines, in the order it defines them, the
    /// first kept of any defined twice.
    pub fn resources(&self) -> Arc<Vec<ResourceKind>> {
        self.kept("resources", || self.read_resources())
    }

    fn read_resources(&self) -> Vec<ResourceKind> {
        resource_kinds(&self.parse_tgis(|_| true), &self.strings(), &self.glyphs())
    }

    /// The resources the resource bar has a cell for, left to right: all
    /// but those that only provide room for another, where the bar's
    /// layout places each. Those it doesn't place come last, in the order
    /// they're defined.
    pub fn economy_bar(&self) -> Vec<Id> {
        // Not in the load lists.
        let parsed = self.parsed(BAR_LAYOUT);
        let bar = (parsed.as_ref()).and_then(|p| {
            p.root
                .blocks_of("Template")
                .find(|t| t.name().is_some_and(|n| Id::from(n) == Id::from(BAR)))
        });
        bar_cells(&self.resources(), bar)
    }

    /// The resources' and the tuning settings' bounds on economies.
    pub fn economy_rules(&self) -> EconomyRules {
        let mut rules = EconomyRules {
            resources: self.resources().to_vec(),
            ..EconomyRules::default()
        };
        let svars = self.svars();
        let first = |of: fn(&defs::SVars) -> Option<f32>| svars.iter().find_map(of);
        if let Some(v) = first(|s| s.economy_limited_resource_max) {
            rules.limited_max = v;
        }
        if let Some(v) = first(|s| s.economy_stockpile_resource_max) {
            rules.stockpile_max = v;
        }
        if let Some(v) = first(|s| s.sell_factor) {
            rules.sell_factor = v;
        }
        if let Some(v) = svars.iter().find_map(|s| s.allow_sell_under_siege) {
            rules.sell_under_siege = v;
        }
        // The match's setting stands in for the tuning one.
        let rule =
            (self.world_rule("ResourceStockpileDefault")).and_then(|r| r.default?.parse().ok());
        if let Some(v) = rule.or_else(|| first(|s| s.resource_stockpile_default)) {
            rules.stockpile_default = v;
        }
        rules
    }

    /// The tuning settings, the game's own (such as `SVars_AA.tgi`), which take
    /// precedence, before the engine's.
    pub(crate) fn svars(&self) -> Arc<Vec<defs::SVars>> {
        self.kept("svars", || self.read_svars())
    }

    fn read_svars(&self) -> Vec<defs::SVars> {
        let versions = self.versions("Game/SVars.tgi");
        let files = self.parse_tgis(|n| versions.iter().any(|v| v == n));
        let files = scoped(&files);
        let mut out: Vec<(Option<usize>, defs::SVars)> = files
            .iter()
            .flat_map(|f| {
                let lower = f.file.to_ascii_lowercase();
                let at = versions.iter().position(|v| *v == lower);
                f.read::<defs::SVars>("SVars").map(move |s| (at, s))
            })
            .collect();
        out.sort_by_key(|(at, _)| *at);
        out.into_iter().map(|(_, s)| s).collect()
    }
}

/// The resources with a cell on the resource bar, in the order `bar`, the
/// bar's layout, places them, as [`GameData::economy_bar`] has them.
fn bar_cells(resources: &[ResourceKind], bar: Option<&tgi::Block>) -> Vec<Id> {
    let offset = |id: &Id| -> Option<f32> {
        let cell = bar?
            .blocks()
            .find(|b| b.kind().is_some_and(|k| Id::from(k) == *id))?;
        cell.blocks_of("View")
            .next()?
            .value("L")?
            .trim()
            .parse()
            .ok()
    };
    let providing: HashSet<&Id> = (resources.iter())
        .filter_map(|r| r.limited_by.as_ref())
        .collect();
    let mut cells: Vec<(Option<f32>, &Id)> = (resources.iter())
        .filter(|r| !providing.contains(&r.id))
        .map(|r| (offset(&r.id), &r.id))
        .collect();
    cells.sort_by(|(a, _), (b, _)| match (a, b) {
        (Some(a), Some(b)) => a.total_cmp(b),
        _ => b.is_some().cmp(&a.is_some()),
    });
    cells.into_iter().map(|(_, id)| id.clone()).collect()
}

/// The `[Resource]` definitions of `files`, in order, the first kept of any
/// defined twice. One without a type, or naming a resource none of them
/// define, is an error in the data. Icons are looked up in `glyphs`.
fn resource_kinds<P: Borrow<tgi::Parsed>>(
    files: &[(String, P)],
    strings: &Strings,
    glyphs: &[defs::Glyph],
) -> Vec<ResourceKind> {
    let mut out: Vec<ResourceKind> = Vec::new();
    for f in &scoped(files) {
        for r in f.read::<defs::Resource>("Resource") {
            let Some(id) = r.ids else { continue };
            if out.iter().any(|k| k.id == id) {
                log::warn!("{}: resource {id} defined again", f.file);
                continue;
            }
            let Some(role) = r.kind else {
                log::warn!("{}: resource {id} has no type", f.file);
                continue;
            };
            out.push(ResourceKind {
                name: localized(strings, r.name.as_deref()).unwrap_or_else(|| id.to_string()),
                description: localized(strings, r.description.as_deref()),
                role,
                limited_by: not_null(r.limited_by),
                purchase_cost: (r.purchase_cost.as_ref())
                    .map(Amounts::from)
                    .unwrap_or_default(),
                glyph: (r.icon.as_deref()).and_then(|i| glyph_image(glyphs, &Id::from(i))),
                icon: r.icon,
                id,
            });
        }
    }
    for r in &out {
        let named = (r.limited_by.iter()).chain(r.purchase_cost.iter().map(|(id, _)| id));
        for id in named {
            if !out.iter().any(|k| &k.id == id) {
                log::warn!("resource {} names {id}, which isn't a resource", r.id);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_add_and_take_by_resource() {
        let mut a = Amounts::default();
        a.set("money", 12.0);
        a.set("AMMO", 5.0);
        let mut b = Amounts::default();
        b.set("money", 2.0);
        b.set("unit_limit_provided", 6.0);
        let sum = a.clone() + &b;
        assert_eq!(sum.get("money"), 14.0);
        assert_eq!(sum.get("unit_limit_provided"), 6.0);
        assert_eq!((sum.clone() - &b).get("money"), 12.0);
        assert_eq!(sum.get("nothing"), 0.0);
        let named: Vec<_> = sum.nonzero().map(|(k, v)| (k.as_str(), v)).collect();
        assert_eq!(
            named,
            [("money", 14.0), ("AMMO", 5.0), ("unit_limit_provided", 6.0)]
        );
        // A resource not named is none of it.
        let mut c = a.clone();
        c.set("oil", 0.0);
        assert_eq!(a, c);
        assert_ne!(a, b);
    }

    #[test]
    fn amounts_read_only_the_numbers_a_block_gives() {
        let parsed = tgi::parse(
            "[Cost]\n{\n\tmoney = 5\n\toil = 0\n\tnote = cheap\n\t[Nested]\n\t{\n\t\tammo = 1\n\t}\n}\n",
        );
        let block = parsed.root.blocks_of("Cost").next().unwrap();
        let read: defs::Resources = tgi::from_block(block).unwrap();
        let amounts = Amounts::from(&read);
        let named: Vec<_> = amounts.iter().map(|(k, v)| (k.as_str(), v)).collect();
        assert_eq!(named, [("money", 5.0)]);
    }

    /// The templates resources are defined with, as AA and Kohan II have
    /// them.
    const TEMPLATES: &str = "\
[template Resource]\n{\n\tstring IDS\n\tlocalized name\n\tstring type\n\tstring limited_by = null\n\tstring icon\n}\n\
[template PurchaseResource inherit=Resource]\n{\n\ttype = STOCKPILED\n}\n\
[template UpkeepResource inherit=Resource]\n{\n\ttype = UPKEEP\n\t[PurchaseCost]\n}\n\
[template LimitedConsumedResource inherit=Resource]\n{\n\ttype = LIMITED_CONSUMED\n}\n";

    fn kinds(resources: &str) -> Vec<ResourceKind> {
        let files = [
            (
                "templates/template_resource.tgi".to_string(),
                tgi::parse(TEMPLATES),
            ),
            ("game/resources.tgi".to_string(), tgi::parse(resources)),
        ];
        let strings: Strings = [("resources_gold_name".to_string(), "Gold".to_string())]
            .into_iter()
            .collect();
        let glyphs = [defs::Glyph {
            ids: Some("ch_treasure_chest".into()),
            index: None,
            display_name: Some("Treasure Chest".into()),
        }];
        resource_kinds(&files, &strings, &glyphs)
    }

    #[test]
    fn resources_and_their_roles_come_from_the_data() {
        let found = kinds(
            "[Resource template=PurchaseResource]\n{\n\tIDS = gold\n\tname = \"#resources_gold_name\"\n\ticon = ch_treasure_chest\n}\n\
             [Resource template=UpkeepResource]\n{\n\tIDS = wood\n\t[PurchaseCost]\n\tgold = 3\n}\n\
             [Resource template=UpkeepResource]\n{\n\tIDS = food_provided\n}\n\
             [Resource template=LimitedConsumedResource]\n{\n\tIDS = food_consumed\n\tlimited_by = food_provided\n}\n\
             [Resource template=PurchaseResource]\n{\n\tIDS = GOLD\n}\n",
        );
        let mut wood_cost = Amounts::default();
        wood_cost.set("gold", 3.0);
        let mut gold = ResourceKind::stockpiled("gold");
        gold.name = "Gold".into();
        gold.icon = Some("ch_treasure_chest".into());
        gold.glyph = Some("TreasureChest".into());
        assert_eq!(
            found,
            [
                gold,
                ResourceKind::upkeep("wood", wood_cost),
                ResourceKind::upkeep("food_provided", Amounts::default()),
                ResourceKind::limited("food_consumed", "food_provided"),
            ]
        );
        let rules = EconomyRules {
            resources: found,
            ..EconomyRules::default()
        };
        let ids = |of: Vec<&Id>| of.into_iter().map(Id::to_string).collect::<Vec<_>>();
        assert_eq!(ids(rules.stockpiled().collect()), ["gold"]);
        let limits: Vec<_> = rules
            .limits()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        assert_eq!(limits, [("food_consumed", "food_provided")]);
    }

    #[test]
    fn the_bar_places_all_but_the_provided_limits_where_its_layout_says() {
        let resources = [
            ResourceKind::stockpiled("money"),
            ResourceKind::upkeep("oil", Amounts::default()),
            ResourceKind::upkeep("unit_limit_provided", Amounts::default()),
            ResourceKind::limited("unit_limit_consumed", "unit_limit_provided"),
            ResourceKind::upkeep("building_limit_provided", Amounts::default()),
            ResourceKind::limited("building_limit_consumed", "building_limit_provided"),
            ResourceKind::upkeep("mana", Amounts::default()),
        ];
        let layout = tgi::parse(
            "[Template EconomyBarWidget]\n{\n\
             \t[Money]\n\t{\n\t\t[View]\n\t\tL = 6\n\t}\n\
             \t[Oil]\n\t{\n\t\t[View]\n\t\tL = 224\n\t}\n\
             \t[Building_Limit_Consumed]\n\t{\n\t\t[View]\n\t\tL = 306\n\t}\n\
             \t[Unit_Limit_Provided]\n\t{\n\t\t[View]\n\t\tL = 100%\n\t}\n\
             \t[Unit_Limit_Consumed]\n\t{\n\t\t[View]\n\t\tL = 381\n\t}\n\
             }\n",
        );
        let bar = layout.root.blocks_of("Template").next();
        let cells: Vec<String> = bar_cells(&resources, bar)
            .iter()
            .map(Id::to_string)
            .collect();
        // Mana, which the layout doesn't place, comes last.
        let placed = [
            "money",
            "oil",
            "building_limit_consumed",
            "unit_limit_consumed",
            "mana",
        ];
        assert_eq!(cells, placed);
        // Without a layout, in the order they're defined.
        let unplaced: Vec<String> = bar_cells(&resources, None)
            .iter()
            .map(Id::to_string)
            .collect();
        let defined = [
            "money",
            "oil",
            "unit_limit_consumed",
            "building_limit_consumed",
            "mana",
        ];
        assert_eq!(unplaced, defined);
    }

    #[test]
    fn a_resource_of_no_known_type_isnt_one() {
        let found = kinds("[Resource template=Resource]\n{\n\tIDS = mana\n\ttype = HOARDED\n}\n");
        assert_eq!(found, []);
    }
}
