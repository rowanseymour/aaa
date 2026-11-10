//! Supply: which buildings give it and over what ground, and how those
//! that only pass it on stay connected to those that give it of
//! themselves.

use aaa_map::{Id, Supply, Zone};

use crate::{Relation, State, TICK, Target, World};

/// Ticks between looks at whether a building gives supply, each building
/// on its own beat.
const BEAT: u64 = (1.0 / TICK) as u64;

/// Where a building stands with supply, as of its last look.
#[derive(Debug, Clone, Default)]
pub struct Supplying {
    /// The ground it supplies, while it does.
    pub zone: Option<Zone>,
    /// For a relay, whether supply reaches it.
    pub connected: bool,
    /// For a relay, the seconds it can go on for once cut off.
    pub stockpile: f32,
    /// Whether it's been looked at since it was put down or changed hands.
    pub(crate) judged: bool,
}

impl World {
    /// How the building at `b` gives supply, if it does.
    pub fn supply_of(&self, b: usize) -> Option<Supply> {
        let template = self.buildings[b].template.as_str();
        self.supplies.get(&Id::from(template)).copied()
    }

    /// Whether a building that passes supply on has none to pass: nothing
    /// reaches it and its stockpile is spent.
    pub fn out_of_supply(&self, b: usize) -> bool {
        let s = &self.buildings[b].supply;
        s.judged && !s.connected && s.stockpile <= 0.0 && self.supply_of(b).is_some_and(|p| p.relay)
    }

    /// Whether `player`'s supply covers `at`: its own zones' and its
    /// allies'.
    pub fn supplied(&self, player: u32, at: [f32; 2]) -> bool {
        self.buildings.iter().any(|b| {
            let zone = b.supply.zone.as_ref();
            zone.is_some_and(|z| self.shares_supply(player, b.owner) && z.holds(at))
        })
    }

    fn shares_supply(&self, a: u32, b: u32) -> bool {
        a == b || self.sides.relation(a, b) == Relation::Ally
    }

    /// Looks at each building's supply: those just put down or taken at
    /// once, the rest on their beats.
    pub(crate) fn supply(&mut self) {
        let all = 0..self.buildings.len();
        let fresh: Vec<usize> = (all.clone())
            .filter(|&b| !self.buildings[b].supply.judged)
            .collect();
        // Each zone laid may connect another relay, so chains standing as
        // a match starts link up without waiting a beat a link.
        for _ in 0..=fresh.len() {
            if !fresh.iter().any(|&b| self.judge(b, false)) {
                break;
            }
        }
        for &b in &fresh {
            self.buildings[b].supply.judged = true;
        }
        for b in all {
            if (self.ticks + b as u64).is_multiple_of(BEAT) && !fresh.contains(&b) {
                self.judge(b, true);
            }
        }
    }

    /// Works out afresh whether the building at `b` gives supply, laying
    /// or lifting its zone to match, and on its beat filling or draining a
    /// relay's stockpile. Returns whether its zone came or went.
    fn judge(&mut self, b: usize, beat: bool) -> bool {
        let Some(supply) = self.supply_of(b) else {
            return false;
        };
        let building = &self.buildings[b];
        let connected = !supply.relay || self.reached(b, &mut Vec::new());
        let mut stockpile = building.supply.stockpile;
        if supply.relay && beat {
            stockpile = match connected {
                true => (stockpile + self.supply_recharge).min(supply.stockpile_max),
                false => stockpile - 1.0,
            }
            .max(0.0);
        }
        let sided = (self.sides.player(building.owner)).is_some_and(|p| !p.independent);
        let gives = sided
            && building.state == State::Standing
            && supply.range >= 0.0
            && (connected || stockpile > 0.0)
            && building.zone.is_none_or(|city| self.held_firmly(b, city));
        let had = building.supply.zone.is_some();
        let laid = (gives && !had).then(|| self.pathing.zone(building.position, supply.range));
        let s = &mut self.buildings[b].supply;
        s.connected = connected;
        s.stockpile = stockpile;
        match gives {
            true => s.zone = laid.or(s.zone.take()),
            false => s.zone = None,
        }
        gives != had
    }

    /// Whether supply reaches the relay at `b`: a zone of another of its
    /// side's or an ally's covers it, laid by what gives supply of itself
    /// or by a relay supply reaches in turn, not counting those in `seen`.
    /// A relay living off its stockpile keeps its zone but passes nothing
    /// on.
    fn reached(&self, b: usize, seen: &mut Vec<usize>) -> bool {
        seen.push(b);
        let owner = self.buildings[b].owner;
        (0..self.buildings.len()).any(|p| {
            let other = &self.buildings[p];
            let covers =
                (other.supply.zone.as_ref()).is_some_and(|z| self.in_zone(z, Target::Building(b)));
            if seen.contains(&p) || !covers || !self.shares_supply(owner, other.owner) {
                return false;
            }
            match self.supply_of(p) {
                Some(s) if s.relay => self.reached(p, seen),
                Some(_) => true,
                None => false,
            }
        })
    }

    /// Whether the owner of the city centred on `c` holds it firmly enough
    /// for it to give supply: its side outweighs its enemies there.
    fn held_firmly(&self, c: usize, zone: f32) -> bool {
        let owner = self.buildings[c].owner;
        let occupants = self.occupants(c, zone);
        let weight = |of: &dyn Fn(u32) -> bool| -> f32 {
            let theirs = occupants.iter().filter(|(o, _)| of(*o));
            theirs.map(|(_, w)| w).sum()
        };
        let enemies = weight(&|o| self.sides.relation(o, owner) == Relation::Enemy);
        weight(&|o| self.shares_supply(owner, o)) > enemies
    }
}
