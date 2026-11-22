//! What each side earns and spends: production and upkeep, the money
//! stockpile, and what an order would do to them.

use std::collections::BTreeMap;
use std::ops::AddAssign;

use tgmap::Id;

pub use tgmap::{Amounts, Economy, EconomyEffect, EconomyRules};

use crate::properties::Held;
use crate::{Commission, Notice, Pause, State, TICK, Target, World};

/// What a side, or one thing of it, earns and takes to keep, per minute.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ledger {
    pub production: Amounts,
    pub upkeep: Amounts,
}

impl AddAssign for Ledger {
    fn add_assign(&mut self, other: Ledger) {
        self.production += other.production;
        self.upkeep += other.upkeep;
    }
}

impl Ledger {
    fn without(mut self, other: &Ledger) -> Ledger {
        self.production -= &other.production;
        self.upkeep -= &other.upkeep;
        self
    }

    /// Where the ledger leaves a side under `rules`.
    pub fn balance(&self, rules: &EconomyRules) -> Balance {
        let money = money(rules);
        let (production, upkeep) = (&self.production, &self.upkeep);
        let mut net = production.clone() - upkeep;
        // What's kept up beyond what's produced is bought, at its price.
        let mut charged = Amounts::default();
        for r in &rules.resources {
            let short = (upkeep.get(r.id.as_str()) - production.get(r.id.as_str())).max(0.0);
            for (by, price) in r.purchase_cost.nonzero() {
                charged.set(by.as_str(), charged.get(by.as_str()) + short * price);
            }
        }
        for (by, amount) in charged.iter() {
            net.set(by.as_str(), net.get(by.as_str()) - amount);
        }
        let income = production.get(money);
        let costs = upkeep.get(money) + charged.get(money);
        let limits = rules
            .limits()
            .map(|(consumed, by)| Limit {
                consumed: consumed.clone(),
                used: upkeep.get(consumed.as_str()),
                provided: production.get(by.as_str()).min(rules.limited_max),
            })
            .collect();
        Balance {
            net,
            income,
            costs,
            limits,
        }
    }
}

/// A side's economy as the resource bar shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Balance {
    /// Each resource's production less upkeep per minute, money's less what
    /// it takes to make up the others' shortfalls.
    pub net: Amounts,
    /// Money earned, and spent with the shortfalls, per minute.
    pub income: f32,
    pub costs: f32,
    /// Each limited resource, with how much is used and allowed.
    pub limits: Vec<Limit>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Limit {
    /// The resource used up, such as `unit_limit_consumed`.
    pub consumed: Id,
    pub used: f32,
    pub provided: f32,
}

/// A side's money, credited as it earns and taken as it spends.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Treasury {
    pub stockpile: f32,
    /// While it's short, how far its income last fell short of its costs
    /// when the side was told, as a percentage.
    pub shortage: Option<f32>,
}

/// How much the shortage has to move by before the side is told again.
const SHORTAGE_STEP: f32 = 5.0;

/// Something that would change a side's economy, to preview.
pub enum Change<'a> {
    /// Commissioning an organization as it's raised, at the building at
    /// this index.
    Commission(usize, &'a tgmap::Organization),
    Research(&'a tgmap::Research),
    /// A packed building, by index into [`World::organizations`], unpacking.
    Unpack(usize),
    /// Putting up a building of this template.
    Build(&'a Id),
}

/// The resource stockpiled and spent, which makes up shortfalls: the first
/// the data defines, as a side keeps one stockpile.
pub(crate) fn money(rules: &EconomyRules) -> &str {
    rules.stockpiled().next().map_or("", Id::as_str)
}

impl World {
    /// What a holding of `owner`'s made from `template` adds to its
    /// economy: its own production and upkeep, as its properties leave them.
    fn holding_ledger(&self, owner: u32, template: &Id) -> Ledger {
        let effects = self.economy_effects(owner, template, &[], []);
        self.affected(template, &effects)
    }

    /// A template's own economy, none for one that has none.
    pub(crate) fn economy_of(&self, template: &Id) -> &Economy {
        static NONE: std::sync::LazyLock<Economy> = std::sync::LazyLock::new(Economy::default);
        self.rules.economies.get(template).unwrap_or(&NONE)
    }

    /// What the properties on a thing of `owner`'s made from `template` do
    /// to the economy, each once: its template's own, its side's
    /// technologies whose `requires` it or one of `with` has, and those it
    /// holds for a time.
    fn economy_effects<'a>(
        &'a self,
        owner: u32,
        template: &Id,
        with: &[&Id],
        held: impl IntoIterator<Item = &'a Held>,
    ) -> Vec<&'a EconomyEffect> {
        let table = &self.rules.economy_effects;
        let kind = |t: &Id| self.rules.kinds.get(t);
        let own = kind(template).into_iter().flat_map(|k| &k.properties);
        let has = |p: &Id| {
            (std::iter::once(template).chain(with.iter().copied()))
                .any(|t| kind(t).is_some_and(|k| k.properties.contains(p)))
        };
        let technologies = (self.technologies_of(owner))
            .filter(|t| table.get(*t).is_some_and(|e| e.requires.iter().all(has)));
        let timed = held.into_iter().map(|h| &h.property);
        let mut ids: Vec<&Id> = Vec::new();
        for id in own.chain(technologies).chain(timed) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids.into_iter().filter_map(|id| table.get(id)).collect()
    }

    /// What a thing earns and keeps under `effects`: its own production
    /// with what they add, if it has an economy, and its upkeep unless one
    /// lifts it.
    fn affected(&self, template: &Id, effects: &[&EconomyEffect]) -> Ledger {
        let Some(e) = self.rules.economies.get(template) else {
            return Ledger::default();
        };
        let added = effects.iter().map(|e| &e.production);
        Ledger {
            production: added.fold(e.production.clone(), |sum, a| sum + a),
            upkeep: match effects.iter().any(|e| e.no_upkeep) {
                true => Amounts::default(),
                false => e.upkeep.clone(),
            },
        }
    }

    /// What an organization adds to its owner's economy: its own production
    /// and that of its members still alive, and its own upkeep and that of
    /// every place in it, the member there or not, but for those that have
    /// unpacked into buildings. Each counts what its properties make of it.
    /// Nothing once it's gone.
    pub fn organization_ledger(&self, organization: usize) -> Ledger {
        let r = &self.organizations[organization];
        if self.destroyed(organization) {
            return Ledger::default();
        }
        let kept: Vec<usize> = (r.units.iter().copied())
            .filter(|&u| !matches!(self.units[u].left, Some(crate::Left::Unpacked(_))))
            .collect();
        // A lone unit is its own organization, and counts once.
        let lone = matches!(&r.units[..], &[u] if self.units[u].template == r.template);
        let mut ledger = Ledger::default();
        if !lone {
            let places: Vec<&Id> = (r.units.iter()).map(|&u| &self.units[u].template).collect();
            let effects = self.economy_effects(r.owner, &r.template, &places, &r.properties);
            ledger += self.affected(&r.template, &effects);
        }
        for u in kept {
            let unit = &self.units[u];
            let held = self.timed_on(Target::Unit(u));
            let effects = self.economy_effects(r.owner, &unit.template, &[], held);
            let mut own = self.affected(&unit.template, &effects);
            if !unit.alive() {
                own.production = Amounts::default();
            }
            ledger += own;
        }
        ledger
    }

    /// What an organization would add to its owner's economy once raised.
    pub(crate) fn commission_ledger(&self, organization: &tgmap::Organization) -> Ledger {
        let members = organization.members.iter().map(|m| &m.template);
        self.members_ledger(&organization.template, members.collect())
    }

    fn members_ledger(&self, template: &Id, members: Vec<&Id>) -> Ledger {
        if members.is_empty() {
            return Ledger::default();
        }
        // A lone unit is its own organization, and counts once.
        let lone = matches!(&members[..], [m] if *m == template);
        let mut ledger = Ledger::default();
        if !lone {
            ledger.upkeep = self.economy_of(template).upkeep.clone();
        }
        for m in members {
            let e = self.economy_of(m);
            ledger.production += &e.production;
            ledger.upkeep += &e.upkeep;
        }
        ledger
    }

    /// What a building counts for in its owner's economy: nothing once
    /// gone, only its upkeep while disabled. The limits taken by what it's
    /// commissioning count for its owner but not for it: see
    /// [`reserved_ledger`](Self::reserved_ledger). One that passes supply on and has none
    /// earns as what it packs into does, or nothing if it needs supply and
    /// doesn't pack. What its properties add or lift is judged by it.
    pub fn building_ledger(&self, building: usize) -> Ledger {
        let b = &self.buildings[building];
        if b.gone() {
            return Ledger::default();
        }
        let e = self.economy_of(&b.template);
        let held = self.timed_on(Target::Building(building));
        let effects = self.economy_effects(b.owner, &b.template, &[], held);
        let upkeep = self.affected(&b.template, &effects).upkeep;
        // Unpacking and packing aren't construction: it earns through both.
        let working = b.state == State::Standing && !b.constructing;
        let own = match self.rules.packing.get(&b.template) {
            _ if !working => None,
            _ if !self.out_of_supply(building) => Some(e.production.clone()),
            Some(packs) => Some(self.commission_ledger(&packs.truck).production),
            None if e.supply_required => None,
            None => Some(e.production.clone()),
        };
        let added = effects.iter().map(|e| &e.production);
        let production = own.map_or_else(Amounts::default, |own| added.fold(own, |sum, a| sum + a));
        Ledger { production, upkeep }
    }

    /// The limits a building's queue has taken for what it's commissioning,
    /// as upkeep: its owner's, until the finished thing holds them.
    pub fn reserved_ledger(&self, building: usize) -> Ledger {
        let b = &self.buildings[building];
        if b.gone() {
            return Ledger::default();
        }
        let mut upkeep = Amounts::default();
        let held = b.queue.iter().filter(|c| c.held);
        for r in held.filter_map(Commission::organization) {
            let taken = self.commission_ledger(r).upkeep;
            for (consumed, _) in self.rules.economy.limits() {
                let consumed = consumed.as_str();
                upkeep.set(consumed, upkeep.get(consumed) + taken.get(consumed));
            }
        }
        Ledger {
            upkeep,
            ..Ledger::default()
        }
    }

    /// Whether a building's queue can't move on this tick: there's nothing
    /// to work on, it's in no state to, or its owner lacks room under a
    /// limit for the first of it. Otherwise the first takes its share of
    /// the limits. Tells the owner when a queue first stops for a reason.
    pub(crate) fn held_up(&mut self, building: usize) -> bool {
        let b = &self.buildings[building];
        if b.gone() || b.busy() {
            return true;
        }
        let Some(first) = b.queue.front() else {
            return true;
        };
        let why = match b.state {
            State::Standing if self.out_of_supply(building) => Some(Pause::Supply),
            State::Standing if first.held => None,
            State::Standing => {
                let full = (first.organization()).and_then(|r| self.over_limit(b.owner, r));
                full.map(Pause::Limit)
            }
            _ => Some(Pause::Disabled),
        };
        let (player, template) = (b.owner, first.template().to_string());
        let b = &mut self.buildings[building];
        let told = std::mem::replace(&mut b.paused, why.clone());
        let held = why.is_some();
        match why {
            Some(why) if told.as_ref() != Some(&why) => self.notices.push(Notice::Paused {
                player,
                building,
                template,
                why,
            }),
            Some(_) => {}
            None => {
                if let Some(first) = b.queue.front_mut() {
                    first.held = true;
                }
            }
        }
        held
    }

    /// Every side's ledger, by player id.
    pub fn ledgers(&self) -> BTreeMap<u32, Ledger> {
        let mut out: BTreeMap<u32, Ledger> = BTreeMap::new();
        for r in 0..self.organizations.len() {
            *out.entry(self.organizations[r].owner).or_default() += self.organization_ledger(r);
        }
        for b in 0..self.buildings.len() {
            let side = out.entry(self.buildings[b].owner).or_default();
            *side += self.building_ledger(b);
            *side += self.reserved_ledger(b);
        }
        for (owner, template) in &self.holdings {
            *out.entry(*owner).or_default() += self.holding_ledger(*owner, template);
        }
        for d in 0..self.deliveries.len() {
            *out.entry(self.deliveries[d].owner).or_default() += self.delivery_ledger(d);
        }
        out
    }

    pub fn ledger(&self, player: u32) -> Ledger {
        self.ledgers().remove(&player).unwrap_or_default()
    }

    pub fn balance(&self, player: u32) -> Balance {
        self.ledger(player).balance(&self.rules.economy)
    }

    /// While `player` can't pay its upkeep, the share of its costs its
    /// income falls short by.
    pub fn short(&self, player: u32) -> Option<f32> {
        self.treasuries.get(&player)?.shortage?;
        let balance = self.balance(player);
        let short = (balance.costs - balance.income) / balance.costs.max(f32::MIN_POSITIVE);
        Some(short.clamp(0.0, 1.0))
    }

    /// How much money `player` has.
    pub fn stockpile(&self, player: u32) -> f32 {
        self.treasuries.get(&player).map_or(0.0, |t| t.stockpile)
    }

    /// Sets how much money `player` has, within the most it can hold.
    pub fn set_stockpile(&mut self, player: u32, amount: f32) {
        let max = self.rules.economy.stockpile_max;
        self.treasuries.entry(player).or_default().stockpile = amount.clamp(0.0, max);
    }

    /// The limit, such as `unit_limit_consumed`, that `organization` would
    /// take `player` over, if any.
    pub(crate) fn over_limit(&self, player: u32, organization: &tgmap::Organization) -> Option<Id> {
        let needs = self.commission_ledger(organization).upkeep;
        let balance = self.balance(player);
        let full = balance.limits.iter().find(|l| {
            let needed = needs.get(l.consumed.as_str());
            needed > 0.0 && l.used + needed > l.provided
        });
        full.map(|l| l.consumed.clone())
    }

    /// The money commissioning `organization` as raised at the building at
    /// `recruiter` takes: its own cost and its members', times what the
    /// recruiter's properties make of what it charges.
    pub fn cost(&self, recruiter: usize, organization: &tgmap::Organization) -> f32 {
        let money = money(&self.rules.economy);
        let members = organization.members.iter().map(|m| &m.template);
        let own = (!organization.lone()).then_some(&organization.template);
        let listed: f32 = (members.chain(own))
            .map(|t| self.economy_of(t).cost.get(money))
            .sum();
        listed * self.recruit_multiplier(recruiter)
    }

    /// What the building at `recruiter` charges for what it recruits is
    /// multiplied by: 1 and what its properties add.
    pub fn recruit_multiplier(&self, recruiter: usize) -> f32 {
        let b = &self.buildings[recruiter];
        let held = self.timed_on(Target::Building(recruiter));
        let effects = self.economy_effects(b.owner, &b.template, &[], held);
        let added: f32 = effects.iter().map(|e| e.recruit_cost).sum();
        (1.0 + added).max(self.rules.supply_rules.multiply_floor)
    }

    /// What researching `item` costs, in the one resource stockpiled.
    pub fn research_cost(&self, item: &tgmap::Research) -> f32 {
        item.cost.get(money(&self.rules.economy))
    }

    /// What `player`'s stockpile and balance would be after `change`.
    pub fn preview(&self, player: u32, change: Change) -> (f32, Balance) {
        let mut ledger = self.ledger(player);
        let mut stockpile = self.stockpile(player);
        match change {
            Change::Commission(recruiter, organization) => {
                stockpile -= self.cost(recruiter, organization);
                ledger += self.commission_ledger(organization);
            }
            Change::Research(item) => stockpile -= self.research_cost(item),
            Change::Build(template) => {
                stockpile -= self.site_cost(template);
                ledger += self.holding_ledger(player, template);
            }
            Change::Unpack(r) => {
                ledger = ledger.without(&self.organization_ledger(r));
                if let Some(u) = &self.organizations[r].unpacks {
                    ledger += self.holding_ledger(player, &u.building.template);
                }
            }
        }
        (stockpile, ledger.balance(&self.rules.economy))
    }

    /// Credits each side's stockpile with a tick's worth of what it nets,
    /// telling it when it runs short and when it no longer is.
    pub(crate) fn earn(&mut self) {
        let ledgers = self.ledgers();
        let max = self.rules.economy.stockpile_max;
        for (&player, treasury) in &mut self.treasuries {
            let balance = ledgers
                .get(&player)
                .cloned()
                .unwrap_or_default()
                .balance(&self.rules.economy);
            let net = balance.net.get(money(&self.rules.economy));
            let after = treasury.stockpile + net * TICK / 60.0;
            treasury.stockpile = after.clamp(0.0, max);
            if after < 0.0 {
                let percent = match balance.costs > 0.0 {
                    true => (balance.costs - balance.income) / balance.costs * 100.0,
                    false => 0.0,
                };
                let told = treasury.shortage;
                if told.is_none_or(|t| (percent - t).abs() > SHORTAGE_STEP) {
                    treasury.shortage = Some(percent);
                    self.notices.push(Notice::Short { player, percent });
                }
            } else if treasury.shortage.take().is_some() {
                self.notices.push(Notice::Repaired { player });
            }
        }
    }
}

crate::hash::digest!(Treasury {
    stockpile,
    shortage
});

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use tgmap::Id;

    use crate::{Change, Command, EconomyEffect, Limit, Notice, Pause, Refused, State, World};

    #[test]
    fn sides_earn_what_they_own_produces_less_its_upkeep() {
        let mut w = economy();
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        w.add_organization(&t);
        let r = company(&mut w, 1);
        company(&mut w, 2);
        let balance = w.balance(1);
        assert_eq!(balance.net.get("money"), 37.0);
        // The company keeps up its own 10 and its three soldiers' 2 each.
        assert_eq!(balance.net.get("ammo"), 25.0 - 16.0);
        assert_eq!(balance.net.get("oil"), 12.0);
        assert_eq!(
            balance.limits,
            [
                Limit {
                    consumed: "unit_limit_consumed".into(),
                    used: 1.0,
                    provided: 6.0
                },
                Limit {
                    consumed: "building_limit_consumed".into(),
                    used: 2.0,
                    provided: 0.0
                },
            ]
        );
        // A lost member's place costs as before, until the organization's gone.
        w.units[w.organizations[r].units[0]].die(0);
        assert_eq!(w.balance(1).net.get("ammo"), 9.0);
        w.destroy(r);
        assert_eq!(w.balance(1).net.get("ammo"), 25.0);
        // A disabled building earns nothing but is still kept up.
        w.buildings[0].state = State::Disabled;
        let ledger = w.ledger(1);
        assert_eq!(ledger.production.get("money"), 12.0);
        assert_eq!(ledger.upkeep.get("building_limit_consumed"), 2.0);
    }

    /// A property doing to the economy what's given, on what has all of
    /// `requires`.
    fn economic(
        production: &[(&str, f32)],
        recruit_cost: f32,
        no_upkeep: bool,
        requires: &[&str],
    ) -> EconomyEffect {
        EconomyEffect {
            production: amounts(production),
            recruit_cost,
            no_upkeep,
            requires: requires.iter().map(|&p| Id::from(p)).collect(),
        }
    }

    #[test]
    fn organizations_earn_for_themselves_as_well_as_their_members() {
        let mut w = economy();
        let r = company(&mut w, 1);
        let money = |w: &World| w.organization_ledger(r).production.get("money");
        assert_eq!(money(&w), 0.0);
        let company = w.rules.economies.get_mut(&Id::from("company")).unwrap();
        company.production.set("money", 5.0);
        assert_eq!(money(&w), 5.0);
        // A lone truck is its own organization, and earns once.
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        let t = w.add_organization(&t);
        assert_eq!(w.organization_ledger(t).production.get("money"), 12.0);
    }

    #[test]
    fn technologies_add_production_to_each_thing_with_what_they_need() {
        let mut w = economy();
        let oil = Id::from("tech_oil_production");
        (w.rules.economy_effects)
            .insert(oil.clone(), economic(&[("oil", 5.0)], 0.0, false, &["oil"]));
        w.rules.kinds.insert(Id::from("house"), kind(&["oil"]));
        let r = company(&mut w, 1);
        w.rules.kinds.insert(Id::from("soldier"), kind(&["oil"]));
        let oil_of = |w: &World| w.balance(1).net.get("oil");
        assert_eq!(oil_of(&w), 10.0);
        w.grant(1, oil.clone());
        // The house and each of the three soldiers, alive or not, and the
        // company for having them.
        assert_eq!(oil_of(&w), 10.0 + 5.0 * 5.0);
        w.units[w.organizations[r].units[0]].die(0);
        assert_eq!(oil_of(&w), 10.0 + 5.0 * 4.0);
        // Unpacking previews the building's extra in place of the truck's.
        w.rules.kinds.insert(Id::from("truck"), kind(&["oil"]));
        w.rules.kinds.insert(Id::from("depot"), kind(&["oil"]));
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        let t = w.add_organization(&t);
        let now = oil_of(&w);
        let (_, after) = w.preview(1, Change::Unpack(t));
        assert_eq!(after.net.get("oil"), now - 2.0 + 5.0);
        // Nothing while the house earns nothing.
        w.buildings[0].state = State::Disabled;
        assert_eq!(w.building_ledger(0).production.get("oil"), 0.0);
        // Not on a side without it.
        assert_eq!(w.balance(2).net.get("oil"), 0.0);
    }

    #[test]
    fn what_has_no_upkeep_uses_no_limits_but_an_organization_keeps_its_places() {
        let mut w = economy();
        let free = Id::from("tech_noupkeep");
        (w.rules.economy_effects).insert(free.clone(), economic(&[], 0.0, true, &[]));
        company(&mut w, 1);
        let before = w.ledger(1).upkeep;
        assert_eq!(before.get("ammo"), 16.0);
        w.grant(1, free);
        let after = w.ledger(1).upkeep;
        // The house's building limit and the company's own upkeep go, and
        // so do the soldiers' own.
        assert_eq!(after.get("building_limit_consumed"), 0.0);
        assert_eq!(after.get("unit_limit_consumed"), 0.0);
        assert_eq!(after.get("ammo"), 0.0);
        assert_eq!(w.balance(1).limits[1].used, 0.0);
    }

    #[test]
    fn an_organizations_lifted_upkeep_leaves_its_members_own() {
        let mut w = economy();
        let free = Id::from("free_company");
        (w.rules.economy_effects).insert(free.clone(), economic(&[], 0.0, true, &[]));
        w.rules
            .kinds
            .insert(Id::from("company"), kind(&["free_company"]));
        company(&mut w, 1);
        let upkeep = w.ledger(1).upkeep;
        assert_eq!(upkeep.get("unit_limit_consumed"), 0.0);
        assert_eq!(upkeep.get("ammo"), 6.0);
    }

    #[test]
    fn recruiters_properties_scale_what_they_charge() {
        let mut w = economy();
        let cheap = Id::from("handicap_easy");
        (w.rules.economy_effects).insert(cheap.clone(), economic(&[], -0.25, false, &[]));
        let dear = Id::from("tech_conscription");
        (w.rules.economy_effects).insert(dear.clone(), economic(&[], 0.5, false, &["infhq"]));
        w.rules.supply_rules.multiply_floor = 0.05;
        w.grant(1, cheap);
        assert_eq!(w.cost(0, &truck()), 187.5);
        // Only where the recruiter has what it needs.
        w.grant(1, dear);
        assert_eq!(w.cost(0, &truck()), 187.5);
        w.rules.kinds.insert(Id::from("house"), kind(&["infhq"]));
        assert_eq!(w.cost(0, &truck()), 312.5);
        // Never below the floor.
        let free = Id::from("free");
        (w.rules.economy_effects).insert(free.clone(), economic(&[], -5.0, false, &[]));
        w.grant(1, free);
        assert_eq!(w.cost(0, &truck()), 250.0 * 0.05);
        // What's paid is what's charged.
        w.apply(1, commission(0, "truck")).unwrap();
        assert!((w.stockpile(1) - (500.0 - 12.5)).abs() < 0.001);
    }

    #[test]
    fn shortfalls_are_bought_with_money() {
        let mut w = economy();
        for _ in 0..4 {
            company(&mut w, 1);
        }
        // 64 ammo kept up against 20 made, so 44 bought at 2 each.
        let balance = w.balance(1);
        assert_eq!(balance.net.get("ammo"), -44.0);
        assert_eq!(balance.net.get("money"), 25.0 - 88.0);
        assert_eq!((balance.income, balance.costs), (25.0, 88.0));
        // Limits provided are capped.
        w.rules
            .economies
            .get_mut(&Id::from("house"))
            .unwrap()
            .production
            .set("unit_limit_provided", 30.0);
        assert_eq!(w.balance(1).limits[0].provided, 20.0);
    }

    #[test]
    fn stockpiles_fill_with_what_sides_net_and_tell_them_when_short() {
        let mut w = economy();
        run(&mut w, 60.0);
        assert!((w.stockpile(1) - 525.0).abs() < 0.01);
        w.set_stockpile(1, 1.0);
        for _ in 0..4 {
            company(&mut w, 1);
        }
        // Netting -63 a minute, 1 lasts under a second.
        run(&mut w, 1.0);
        assert_eq!(w.stockpile(1), 0.0);
        let percent = (88.0 - 25.0) / 88.0 * 100.0;
        assert_eq!(w.notices, [Notice::Short { player: 1, percent }]);
        // Told again only once it moves on by more than 5 points.
        let earn = |w: &mut World, money| {
            let house = w.rules.economies.get_mut(&Id::from("house")).unwrap();
            house.production.set("money", money);
            run(w, 1.0);
        };
        earn(&mut w, 27.0);
        assert_eq!(w.notices.len(), 1);
        earn(&mut w, 15.0);
        assert_eq!(w.notices.len(), 2);
        w.set_stockpile(1, 99999.0);
        run(&mut w, 1.0);
        assert_eq!(w.notices[2], Notice::Repaired { player: 1 });
        // Never more than the most a stockpile holds.
        w.set_stockpile(1, 200000.0);
        assert_eq!(w.stockpile(1), 99999.0);
    }

    #[test]
    fn commissions_are_paid_for_up_front_and_refunded_if_called_off() {
        let mut w = economy();
        w.rules.recruit_times.insert(Id::from("truck"), 10.0);
        let order = |w: &mut World| w.apply(1, commission(0, "truck"));
        order(&mut w).unwrap();
        order(&mut w).unwrap();
        assert_eq!(w.stockpile(1), 0.0);
        assert_eq!(order(&mut w), Err(Refused::Unaffordable));
        let second = w.buildings[0].queue[1].id;
        w.apply(
            1,
            Command::CancelCommission {
                building: 0,
                commission: second,
            },
        )
        .unwrap();
        assert_eq!(w.stockpile(1), 250.0);
    }

    #[test]
    fn previews_show_the_economy_after_an_order() {
        let mut w = economy();
        let (stockpile, balance) = w.preview(1, Change::Commission(0, &truck()));
        assert_eq!(stockpile, 250.0);
        assert_eq!(balance.net.get("money"), 37.0);
        assert_eq!(balance.limits[0].provided, 6.0);
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_organization(&t);
        // Unpacking swaps the truck's production for the depot's.
        let (stockpile, balance) = w.preview(1, Change::Unpack(r));
        assert_eq!(stockpile, 500.0);
        assert_eq!(balance.net.get("money"), 50.0);
        assert_eq!(balance.limits[1].used, 2.0);
    }

    #[test]
    fn start_locations_give_their_side_room_to_build() {
        let mut w = economy();
        w.rules.economies.insert(
            Id::from("start_loc"),
            tgmap::Economy {
                production: amounts(&[("building_limit_provided", 20.0)]),
                ..Default::default()
            },
        );
        w.holdings.push((1, "start_loc".into()));
        let building = w.balance(1).limits[1].clone();
        assert_eq!((building.used, building.provided), (1.0, 20.0));
    }

    #[test]
    fn commissions_wait_at_the_front_of_the_queue_for_room_under_limits() {
        let mut w = economy();
        let company = raised_company(&mut w, 1);
        recruits(&mut w, "house", company, 1.0);
        let order = commission(0, "company");
        // Accepted and paid for though there's no room.
        w.apply(1, order).unwrap();
        for _ in 0..20 {
            w.tick();
        }
        assert_eq!(w.buildings[0].progress, 0);
        assert_eq!(
            w.buildings[0].paused,
            Some(Pause::Limit("unit_limit_consumed".into()))
        );
        let paused = |w: &World| {
            w.notices
                .iter()
                .filter(|n| matches!(n, Notice::Paused { player: 1, .. }))
                .count()
        };
        assert_eq!(paused(&w), 1, "told once");
        // Room once a truck provides it, taken while it's raised.
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        w.add_organization(&t);
        w.tick();
        assert_eq!(w.buildings[0].paused, None);
        assert_eq!(w.balance(1).limits[0].used, 1.0);
        let own = w.building_ledger(0).upkeep.get("unit_limit_consumed");
        assert_eq!(own, 0.0, "the side's, not the building's own");
        for _ in 0..10 {
            w.tick();
        }
        assert!(w.buildings[0].queue.is_empty());
        assert_eq!(
            w.balance(1).limits[0].used,
            1.0,
            "the raised company holds it"
        );
    }

    #[test]
    fn disabled_buildings_hold_their_queues() {
        let mut w = economy();
        let order = commission(0, "truck");
        w.apply(1, order).unwrap();
        w.tick();
        w.buildings[0].state = State::Disabled;
        for _ in 0..20 {
            w.tick();
        }
        assert_eq!(w.buildings[0].queue.len(), 1);
        assert_eq!(w.buildings[0].paused, Some(Pause::Disabled));
        assert!(w.notices.iter().any(|n| matches!(
            n,
            Notice::Paused {
                why: Pause::Disabled,
                ..
            }
        )));
    }

    #[test]
    fn queues_are_refunded_to_their_owner_when_their_building_changes_hands() {
        let mut w = economy();
        w.rules.recruit_times.insert(Id::from("truck"), 10.0);
        let order = commission(0, "truck");
        w.apply(1, order).unwrap();
        assert_eq!(w.stockpile(1), 250.0);
        w.change_hands(0, 2);
        assert!(w.buildings[0].queue.is_empty());
        assert_eq!(w.stockpile(1), 500.0);
        assert_eq!(w.stockpile(2), 0.0);
    }
}
