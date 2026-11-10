//! What sides earn and spend: the resources, each template's own cost,
//! production and upkeep, and the settings that bound them.

use std::collections::HashMap;
use std::ops::{Add, AddAssign, Sub, SubAssign};

use crate::{GameData, Id, defs, scoped};

/// The resources the data's costs, upkeeps and production name, by field
/// name, in the order [`Amounts`] keeps them.
pub const RESOURCES: [&str; 7] = [
    "money",
    "ammo",
    "oil",
    "unit_limit_consumed",
    "building_limit_consumed",
    "unit_limit_provided",
    "building_limit_provided",
];

/// So much of each of the [`RESOURCES`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Amounts(pub [f32; RESOURCES.len()]);

impl Amounts {
    /// The resource's place in [`RESOURCES`].
    pub fn index(kind: &str) -> Option<usize> {
        RESOURCES.iter().position(|r| r.eq_ignore_ascii_case(kind))
    }

    /// How much there is of a resource, 0 for one it doesn't know.
    pub fn get(&self, kind: &str) -> f32 {
        Self::index(kind).map_or(0.0, |i| self.0[i])
    }

    pub fn set(&mut self, kind: &str, amount: f32) {
        if let Some(i) = Self::index(kind) {
            self.0[i] = amount;
        }
    }

    /// Each resource there's any of, by field name.
    pub fn nonzero(&self) -> impl Iterator<Item = (&'static str, f32)> + '_ {
        RESOURCES.into_iter().zip(self.0).filter(|&(_, a)| a != 0.0)
    }
}

impl From<&defs::Resources> for Amounts {
    fn from(r: &defs::Resources) -> Self {
        let mut out = Amounts::default();
        for (kind, amount) in r.amounts() {
            out.set(kind, amount);
        }
        out
    }
}

impl AddAssign for Amounts {
    fn add_assign(&mut self, other: Amounts) {
        for (a, b) in self.0.iter_mut().zip(other.0) {
            *a += b;
        }
    }
}

impl SubAssign for Amounts {
    fn sub_assign(&mut self, other: Amounts) {
        for (a, b) in self.0.iter_mut().zip(other.0) {
            *a -= b;
        }
    }
}

impl Add for Amounts {
    type Output = Amounts;
    fn add(mut self, other: Amounts) -> Amounts {
        self += other;
        self
    }
}

impl Sub for Amounts {
    type Output = Amounts;
    fn sub(mut self, other: Amounts) -> Amounts {
        self -= other;
        self
    }
}

/// A template's own economy, as its data gives it, not counting its
/// members'.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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

/// The settings that bound every side's economy.
#[derive(Debug, Clone, PartialEq)]
pub struct EconomyRules {
    /// What making up a shortfall of a unit of each resource costs, in
    /// money; 0 for those that can't be bought.
    pub purchase_cost: Amounts,
    /// The resources that are stockpiled.
    pub stockpiled: Vec<&'static str>,
    /// Each resource held to another, consumed by provided.
    pub limits: Vec<(&'static str, &'static str)>,
    /// The most a limiting resource provides, in total.
    pub limited_max: f32,
    /// The most a stockpile holds.
    pub stockpile_max: f32,
    /// What a stockpile starts at for sides that take the default, on maps
    /// that don't set it.
    pub stockpile_default: f32,
}

impl Default for EconomyRules {
    fn default() -> Self {
        EconomyRules {
            purchase_cost: Amounts::default(),
            stockpiled: vec!["money"],
            limits: Vec::new(),
            limited_max: f32::INFINITY,
            stockpile_max: f32::INFINITY,
            stockpile_default: 0.0,
        }
    }
}

impl GameData {
    /// Each template's own [`Economy`], for those that have one, by
    /// template id.
    pub fn economies(&mut self) -> HashMap<Id, Economy> {
        self.by_template(|t| {
            let e = t.economy_component.as_ref()?;
            let of = |r: Option<&defs::Resources>| r.map(Amounts::from).unwrap_or_default();
            Some(Economy {
                cost: of(e.cost.as_ref()),
                production: of(e.production.as_ref()),
                upkeep: of(e.upkeep.as_ref()),
                supply_required: e.supply_required.unwrap_or(false),
            })
        })
    }

    /// Each template's [`Supply`], for those that give it, by template id.
    pub fn supplies(&mut self) -> HashMap<Id, Supply> {
        self.by_template(|t| {
            let s = t.supply_component.as_ref()?;
            Some(Supply {
                range: s.supply_range?,
                relay: s.is_relay.unwrap_or(false),
                stockpile_max: s.stockpile_max.unwrap_or(0.0),
            })
        })
    }

    /// Seconds a connected relay's stockpile gains each second.
    pub fn supply_recharge(&mut self) -> f32 {
        let svars = self.svars();
        (svars.iter())
            .find_map(|v| v.supply_stockpile_recharge_rate)
            .unwrap_or(1.0)
    }

    /// The resources' and the tuning settings' bounds on economies.
    pub fn economy_rules(&mut self) -> EconomyRules {
        let files = self.parse_tgis(|n| {
            n == "game/resources.tgi"
                || n == "game/world_rules_aa.tgi"
                || n.starts_with("templates/")
        });
        let files = scoped(&files);
        let resources: Vec<defs::Resource> = files
            .iter()
            .flat_map(|f| f.read::<defs::Resource>("Resource"))
            .collect();
        let known = |id: &Id| {
            RESOURCES
                .into_iter()
                .find(|r| id.as_str().eq_ignore_ascii_case(r))
        };
        let mut rules = EconomyRules::default();
        let mut stockpiled = Vec::new();
        for r in &resources {
            let Some(kind) = r.ids.as_ref().and_then(known) else {
                continue;
            };
            match &r.purchase_cost {
                Some(cost) => rules.purchase_cost.set(kind, cost.money.unwrap_or(0.0)),
                // Resources bought with nothing else are what's stockpiled.
                None if r.limited_by.is_none() => stockpiled.push(kind),
                None => {}
            }
            if let Some(by) = r.limited_by.as_ref().and_then(known) {
                rules.limits.push((kind, by));
            }
        }
        if !stockpiled.is_empty() {
            rules.stockpiled = stockpiled;
        }
        let svars = self.svars();
        let first = |of: fn(&defs::SVars) -> Option<f32>| svars.iter().find_map(of);
        if let Some(v) = first(|s| s.economy_limited_resource_max) {
            rules.limited_max = v;
        }
        if let Some(v) = first(|s| s.economy_stockpile_resource_max) {
            rules.stockpile_max = v;
        }
        // The match's setting stands in for the tuning one.
        let rule = files
            .iter()
            .flat_map(|f| f.read::<defs::WorldRule>("WorldRule"))
            .find(|r| {
                r.ids
                    .as_ref()
                    .is_some_and(|id| id.as_str().eq_ignore_ascii_case("ResourceStockpileDefault"))
            })
            .and_then(|r| r.default?.parse().ok());
        if let Some(v) = rule.or_else(|| first(|s| s.resource_stockpile_default)) {
            rules.stockpile_default = v;
        }
        rules
    }

    /// The tuning settings, the game's own (`SVars_AA.tgi`), which take
    /// precedence, before the engine's.
    pub(crate) fn svars(&mut self) -> Vec<defs::SVars> {
        let files = self.parse_tgis(|n| n == "game/svars_aa.tgi" || n == "game/svars.tgi");
        let files = scoped(&files);
        let mut out: Vec<(bool, defs::SVars)> = files
            .iter()
            .flat_map(|f| {
                let own = f.file.to_ascii_lowercase().ends_with("svars_aa.tgi");
                f.read::<defs::SVars>("SVars").map(move |s| (own, s))
            })
            .collect();
        out.sort_by_key(|(own, _)| !own);
        out.into_iter().map(|(_, s)| s).collect()
    }
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
        let sum = a + b;
        assert_eq!(sum.get("money"), 14.0);
        assert_eq!(sum.get("unit_limit_provided"), 6.0);
        assert_eq!((sum - b).get("money"), 12.0);
        assert_eq!(sum.get("nothing"), 0.0);
        assert_eq!(
            sum.nonzero().collect::<Vec<_>>(),
            [("money", 14.0), ("ammo", 5.0), ("unit_limit_provided", 6.0)]
        );
    }
}
