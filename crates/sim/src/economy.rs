//! What each side earns and spends: production and upkeep, the money
//! stockpile, and what an order would do to them.

use std::collections::BTreeMap;
use std::ops::AddAssign;

use aaa_map::Id;

pub use aaa_map::{Amounts, Economy, EconomyRules};

use crate::{Commission, Pause, State, TICK, World};

/// What a side, or one thing of it, earns and takes to keep, per minute.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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
    fn without(mut self, other: Ledger) -> Ledger {
        self.production -= other.production;
        self.upkeep -= other.upkeep;
        self
    }

    /// Where the ledger leaves a side under `rules`.
    pub fn balance(&self, rules: &EconomyRules) -> Balance {
        let money = money(rules);
        let (production, upkeep) = (self.production, self.upkeep);
        let mut net = production - upkeep;
        // What's kept up beyond what's produced is bought with money.
        let charged: f32 = (0..net.0.len())
            .map(|i| (upkeep.0[i] - production.0[i]).max(0.0) * rules.purchase_cost.0[i])
            .sum();
        let income = production.get(money);
        let costs = upkeep.get(money) + charged;
        net.set(money, income - costs);
        let limits = rules
            .limits
            .iter()
            .map(|&(consumed, by)| Limit {
                consumed,
                used: upkeep.get(consumed),
                provided: production.get(by).min(rules.limited_max),
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limit {
    /// The resource used up, such as `unit_limit_consumed`.
    pub consumed: &'static str,
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

/// Something a side is told.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// Its stockpile has run out, its income `percent` short of its costs.
    Short { player: u32, percent: f32 },
    /// It's no longer short.
    Repaired { player: u32 },
    /// A building of its, by index into [`World::buildings`], has stopped
    /// work on the regiment of `template` at the front of its queue.
    Paused {
        player: u32,
        building: usize,
        template: String,
        why: Pause,
    },
    /// A packed building of its, by index into [`World::regiments`],
    /// couldn't unpack where it was sent.
    CannotUnpack { player: u32, regiment: usize },
    /// A regiment of its, by index into [`World::regiments`], was ordered
    /// to the HQ with this id.
    Attached {
        player: u32,
        hq: u32,
        regiment: usize,
    },
    /// A regiment of its was ordered away from the HQ with this id.
    Detached {
        player: u32,
        hq: u32,
        regiment: usize,
    },
    /// A building of its, by index into [`World::buildings`], was taken by
    /// player `by`; for a city, its centre.
    Lost {
        player: u32,
        building: usize,
        by: u32,
    },
    /// It took a building, by index into [`World::buildings`]; for a city,
    /// its centre.
    Captured { player: u32, building: usize },
    /// A building of its, by index into [`World::buildings`], has put out
    /// the regiment at this index into [`World::regiments`].
    PutOut {
        player: u32,
        building: usize,
        regiment: usize,
    },
    /// A building of its, by index into [`World::buildings`], has finished
    /// researching `technology`.
    Researched {
        player: u32,
        building: usize,
        technology: Id,
    },
    /// It holds a technology it didn't.
    Gained { player: u32, technology: Id },
    /// Something of `owner`'s is destroyed, by whatever cause.
    Destroyed { owner: u32, what: Fallen },
    /// A building of its, by index into [`World::buildings`], has been
    /// sold, or destroyed when it could have been, for `paid` money.
    Sold {
        player: u32,
        building: usize,
        paid: f32,
    },
}

/// A regiment or building destroyed, by index into [`World::regiments`]
/// or [`World::buildings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallen {
    Regiment(usize),
    Building(usize),
}

/// How much the shortage has to move by before the side is told again.
const SHORTAGE_STEP: f32 = 5.0;

/// Something that would change a side's economy, to preview.
pub enum Change<'a> {
    /// Commissioning a regiment as it's raised.
    Commission(&'a aaa_map::Regiment),
    Research(&'a aaa_map::Research),
    /// A packed building, by index into [`World::regiments`], unpacking.
    Unpack(usize),
}

/// The resource stockpiled and spent, which makes up shortfalls.
pub(crate) fn money(rules: &EconomyRules) -> &'static str {
    rules.stockpiled.first().copied().unwrap_or("money")
}

impl World {
    /// What a side's holdings add to its economy: their own production and
    /// upkeep.
    fn holding_ledger(&self, template: &str) -> Ledger {
        let e = self.economy_of(template);
        Ledger {
            production: e.production,
            upkeep: e.upkeep,
        }
    }

    /// A template's own economy, none for one that has none.
    pub(crate) fn economy_of(&self, template: &str) -> Economy {
        self.economies
            .get(&Id::from(template))
            .copied()
            .unwrap_or_default()
    }

    /// What a regiment adds to its owner's economy: its members'
    /// production, and its own upkeep and that of every place in it, the
    /// member there or not, but for those that have unpacked into
    /// buildings. Nothing once it's gone.
    pub fn regiment_ledger(&self, regiment: usize) -> Ledger {
        let r = &self.regiments[regiment];
        if self.destroyed(regiment) {
            return Ledger::default();
        }
        let kept = (r.units.iter())
            .filter(|&&u| !matches!(self.units[u].left, Some(crate::Left::Unpacked(_))));
        let templates = kept.map(|&u| &self.units[u].template).collect();
        let mut ledger = self.organization_ledger(&r.template, templates);
        let earning = self
            .survivors(regiment)
            .map(|u| self.economy_of(&self.units[u].template));
        ledger.production = earning.fold(Amounts::default(), |sum, e| sum + e.production);
        ledger
    }

    /// What a regiment would add to its owner's economy once raised.
    fn commission_ledger(&self, regiment: &aaa_map::Regiment) -> Ledger {
        let members = regiment.members.iter().map(|m| &m.template);
        self.organization_ledger(&regiment.template, members.collect())
    }

    fn organization_ledger(&self, template: &str, members: Vec<&String>) -> Ledger {
        if members.is_empty() {
            return Ledger::default();
        }
        // A lone unit is its own regiment, and counts once.
        let lone = matches!(&members[..], [m] if m.as_str() == template);
        let mut ledger = Ledger::default();
        if !lone {
            ledger.upkeep = self.economy_of(template).upkeep;
        }
        for m in members {
            let e = self.economy_of(m);
            ledger.production += e.production;
            ledger.upkeep += e.upkeep;
        }
        ledger
    }

    /// What a building adds to its owner's economy: nothing once gone,
    /// only its upkeep while disabled, and the limits taken by
    /// what it's commissioning. One that passes supply on and has none
    /// earns as what it packs into does, or nothing if it needs supply and
    /// doesn't pack.
    pub fn building_ledger(&self, building: usize) -> Ledger {
        let b = &self.buildings[building];
        if b.gone() {
            return Ledger::default();
        }
        let e = self.economy_of(&b.template);
        let mut upkeep = e.upkeep;
        for r in b
            .queue
            .iter()
            .filter(|c| c.held)
            .filter_map(Commission::regiment)
        {
            let taken = self.commission_ledger(r).upkeep;
            for &(consumed, _) in &self.economy.limits {
                upkeep.set(consumed, upkeep.get(consumed) + taken.get(consumed));
            }
        }
        // Unpacking and packing aren't construction: it earns through both.
        let working = b.state == State::Standing;
        let production = match self.packing.get(&b.template) {
            _ if !working => Amounts::default(),
            _ if !self.out_of_supply(building) => e.production,
            Some(packs) => self.commission_ledger(&packs.truck).production,
            None if e.supply_required => Amounts::default(),
            None => e.production,
        };
        Ledger { production, upkeep }
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
        // A regiment needs somewhere to come out.
        if first.regiment().is_some() && b.deploy.is_none() {
            return true;
        }
        let why = match b.state {
            State::Standing if self.out_of_supply(building) => Some(Pause::Supply),
            State::Standing if first.held => None,
            State::Standing => {
                let balance = self.balance(b.owner);
                let needs = first.regiment().map(|r| self.commission_ledger(r).upkeep);
                let needs = needs.unwrap_or_default();
                let full = balance.limits.iter().find(|l| {
                    needs.get(l.consumed) > 0.0 && l.used + needs.get(l.consumed) > l.provided
                });
                full.map(|l| Pause::Limit(l.consumed))
            }
            _ => Some(Pause::Disabled),
        };
        let (player, template) = (b.owner, first.template().to_string());
        let b = &mut self.buildings[building];
        let told = b.paused;
        b.paused = why;
        match why {
            Some(why) if told != Some(why) => self.notices.push(Notice::Paused {
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
        why.is_some()
    }

    /// Every side's ledger, by player id.
    pub fn ledgers(&self) -> BTreeMap<u32, Ledger> {
        let mut out: BTreeMap<u32, Ledger> = BTreeMap::new();
        for r in 0..self.regiments.len() {
            *out.entry(self.regiments[r].owner).or_default() += self.regiment_ledger(r);
        }
        for b in 0..self.buildings.len() {
            *out.entry(self.buildings[b].owner).or_default() += self.building_ledger(b);
        }
        for (owner, template) in &self.holdings {
            *out.entry(*owner).or_default() += self.holding_ledger(template);
        }
        out
    }

    pub fn ledger(&self, player: u32) -> Ledger {
        self.ledgers().remove(&player).unwrap_or_default()
    }

    pub fn balance(&self, player: u32) -> Balance {
        self.ledger(player).balance(&self.economy)
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
        let max = self.economy.stockpile_max;
        self.treasuries.entry(player).or_default().stockpile = amount.clamp(0.0, max);
    }

    /// The money commissioning `regiment` as raised takes: its own cost and
    /// its members'.
    pub fn cost(&self, regiment: &aaa_map::Regiment) -> f32 {
        let money = money(&self.economy);
        let members = regiment.members.iter().map(|m| m.template.as_str());
        let mut templates: Vec<&str> = members.collect();
        if !regiment.lone() {
            templates.push(&regiment.template);
        }
        templates
            .into_iter()
            .map(|t| self.economy_of(t).cost.get(money))
            .sum()
    }

    /// What researching `item` costs, in the one resource stockpiled.
    pub fn research_cost(&self, item: &aaa_map::Research) -> f32 {
        item.cost.get(money(&self.economy))
    }

    /// What `player`'s stockpile and balance would be after `change`.
    pub fn preview(&self, player: u32, change: Change) -> (f32, Balance) {
        let mut ledger = self.ledger(player);
        let mut stockpile = self.stockpile(player);
        match change {
            Change::Commission(regiment) => {
                stockpile -= self.cost(regiment);
                ledger += self.commission_ledger(regiment);
            }
            Change::Research(item) => stockpile -= self.research_cost(item),
            Change::Unpack(r) => {
                ledger = ledger.without(self.regiment_ledger(r));
                if let Some(u) = &self.regiments[r].unpacks {
                    let e = self.economy_of(&u.building.template);
                    ledger += Ledger {
                        production: e.production,
                        upkeep: e.upkeep,
                    };
                }
            }
        }
        (stockpile, ledger.balance(&self.economy))
    }

    /// Credits each side's stockpile with a tick's worth of what it nets,
    /// telling it when it runs short and when it no longer is.
    pub(crate) fn earn(&mut self) {
        let ledgers = self.ledgers();
        let max = self.economy.stockpile_max;
        for (&player, treasury) in &mut self.treasuries {
            let balance = ledgers
                .get(&player)
                .copied()
                .unwrap_or_default()
                .balance(&self.economy);
            let net = balance.net.get(money(&self.economy));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;
    use aaa_map::Role;

    use crate::setup::holdings;
    use crate::*;

    #[test]
    fn sides_earn_what_they_own_produces_less_its_upkeep() {
        let mut w = economy();
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        w.add_regiment(&t);
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
                    consumed: "unit_limit_consumed",
                    used: 1.0,
                    provided: 6.0
                },
                Limit {
                    consumed: "building_limit_consumed",
                    used: 2.0,
                    provided: 0.0
                },
            ]
        );
        // A lost member's place costs as before, until the regiment's gone.
        w.units[w.regiments[r].units[0]].die(0);
        assert_eq!(w.balance(1).net.get("ammo"), 9.0);
        w.destroy(r);
        assert_eq!(w.balance(1).net.get("ammo"), 25.0);
        // A disabled building earns nothing but is still kept up.
        w.buildings[0].state = State::Disabled;
        let ledger = w.ledger(1);
        assert_eq!(ledger.production.get("money"), 12.0);
        assert_eq!(ledger.upkeep.get("building_limit_consumed"), 2.0);
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
        w.economies
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
            let house = w.economies.get_mut(&Id::from("house")).unwrap();
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
        let order = |w: &mut World| {
            w.apply(
                1,
                Command::Commission {
                    building: 0,
                    commission: Box::new(Commission::new(truck(), 10.0)),
                },
            )
        };
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
        let (stockpile, balance) = w.preview(1, Change::Commission(&truck()));
        assert_eq!(stockpile, 250.0);
        assert_eq!(balance.net.get("money"), 37.0);
        assert_eq!(balance.limits[0].provided, 6.0);
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        // Unpacking swaps the truck's production for the depot's.
        let (stockpile, balance) = w.preview(1, Change::Unpack(r));
        assert_eq!(stockpile, 500.0);
        assert_eq!(balance.net.get("money"), 50.0);
        assert_eq!(balance.limits[1].used, 2.0);
    }

    #[test]
    fn start_locations_give_their_side_room_to_build() {
        let mut w = economy();
        w.economies.insert(
            Id::from("start_loc"),
            aaa_map::Economy {
                production: amounts(&[("building_limit_provided", 20.0)]),
                ..Default::default()
            },
        );
        w.holdings.push((1, "start_loc".into()));
        let building = w.balance(1).limits[1];
        assert_eq!((building.used, building.provided), (1.0, 20.0));
    }

    #[test]
    fn what_a_map_leaves_out_for_lack_of_room_adds_nothing() {
        let mut w = economy();
        w.economies.insert(
            Id::from("start_loc"),
            aaa_map::Economy {
                production: amounts(&[("building_limit_provided", 20.0)]),
                ..Default::default()
            },
        );
        let before = w.balance(1);
        w.holdings = holdings([
            (Role::Crowded, Some(1), "start_loc"),
            (Role::Other, None, "start_loc"),
        ]);
        assert!(w.holdings.is_empty());
        assert_eq!(w.balance(1), before);
        w.holdings = holdings([(Role::Other, Some(1), "start_loc")]);
        assert_ne!(w.balance(1), before);
    }

    #[test]
    fn commissions_wait_at_the_front_of_the_queue_for_room_under_limits() {
        let mut w = economy();
        let company = raised_company(&mut w, 1);
        // An order can't say it has its share of the limits already.
        let mut commission = Commission::new(company, 1.0);
        commission.held = true;
        let order = Command::Commission {
            building: 0,
            commission: Box::new(commission),
        };
        // Accepted and paid for though there's no room.
        w.apply(1, order).unwrap();
        for _ in 0..20 {
            w.tick();
        }
        assert_eq!(w.buildings[0].progress, 0);
        assert_eq!(
            w.buildings[0].paused,
            Some(Pause::Limit("unit_limit_consumed"))
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
        w.add_regiment(&t);
        w.tick();
        assert_eq!(w.buildings[0].paused, None);
        assert_eq!(w.balance(1).limits[0].used, 1.0);
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
        let order = Command::Commission {
            building: 0,
            commission: Box::new(Commission::new(truck(), 1.0)),
        };
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
        let order = Command::Commission {
            building: 0,
            commission: Box::new(Commission::new(truck(), 10.0)),
        };
        w.apply(1, order).unwrap();
        assert_eq!(w.stockpile(1), 250.0);
        w.change_hands(0, 2);
        assert!(w.buildings[0].queue.is_empty());
        assert_eq!(w.stockpile(1), 500.0);
        assert_eq!(w.stockpile(2), 0.0);
    }
}
