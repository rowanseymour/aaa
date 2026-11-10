//! Supply: which buildings give it and over what ground, and how those
//! that only pass it on stay connected to those that give it of
//! themselves.

use std::collections::BTreeMap;

use aaa_map::{Id, Supply, Zone};

use crate::{Building, Enemy, Order, Relation, State, TICK, Target, World, deploy_point, distance};

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

    pub(crate) fn shares_supply(&self, a: u32, b: u32) -> bool {
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

/// The least and most of a second's attrition a member takes, as a share
/// of its regiment's rate.
const ATTRITION: (f32, f32) = (0.75, 1.75);

impl World {
    /// Where a regiment is for supply: where its leader is, or, with its
    /// leader lost, the middle of those left.
    pub fn leader_position(&self, r: usize) -> [f32; 2] {
        let regiment = &self.regiments[r];
        let leader = regiment.leader.map(|p| &self.units[regiment.units[p]]);
        match leader.filter(|u| u.alive()) {
            Some(u) => u.position,
            None => self.centre(r),
        }
    }

    /// Whether anything has the regiment at `r` as a target of its
    /// engagement.
    fn engaged_by(&self, r: usize) -> bool {
        let them = Enemy::Regiment(r);
        (0..self.regiments.len())
            .any(|e| self.regiments[e].engaged.contains(&them) && !self.destroyed(e))
            || (self.buildings.iter()).any(|b| b.standing() && b.noticed.contains(&them))
    }

    /// Looks at each regiment's supply on its beat: those in supply heal
    /// and fill their ranks, and those out of it wear away while their
    /// side can't pay its upkeep.
    pub(crate) fn resupply(&mut self) {
        // Worked out once for all those looked at this tick.
        let mut hqs = None;
        let mut shorts: BTreeMap<u32, Option<f32>> = BTreeMap::new();
        for r in 0..self.regiments.len() {
            if !(self.ticks + r as u64).is_multiple_of(BEAT) {
                continue;
            }
            let regiment = &self.regiments[r];
            let used = self.logistics_of(&regiment.template);
            let Some(logistics) = used.filter(|l| l.supply_used && !self.destroyed(r)) else {
                self.regiments[r].in_supply = false;
                continue;
            };
            let hqs = hqs.get_or_insert_with(|| self.hqs());
            self.keep_attached(r, hqs);
            let regiment = &self.regiments[r];
            let owner = regiment.owner;
            let sided = self.sides.player(owner).is_some_and(|p| !p.independent);
            let in_supply = sided
                && !self.engaged_by(r)
                && (regiment.hq.is_some() || logistics.detached)
                && self.supplied(owner, self.leader_position(r));
            let resting =
                regiment.engaged.is_empty() && !matches!(regiment.order, Order::Pursue(_));
            self.regiments[r].in_supply = in_supply;
            let short = *shorts.entry(owner).or_insert_with(|| self.short(owner));
            match (in_supply, short) {
                (true, _) if resting => self.replenish(r, 1.0 - short.unwrap_or(0.0)),
                (false, Some(_)) if sided => self.wear(r, logistics.shortfall_damage),
                _ => {}
            }
        }
    }

    /// A second of resupply for the regiment at `r`, at `pace` of the
    /// usual: its leader first, then the first of its members hurt, then
    /// the first lost.
    fn replenish(&mut self, r: usize, pace: f32) {
        let regiment = &self.regiments[r];
        let rate = |u: usize| {
            let unit = &self.units[u];
            let rate = self.resupply_rates.get(&Id::from(unit.template.as_str()));
            let health = unit.fighter.as_ref().map_or(0.0, |f| f.health);
            (rate.copied().unwrap_or(0.0) * pace.max(0.0), health)
        };
        let hurt = |&p: &usize| {
            let (u, (rate, health)) = (regiment.units[p], rate(regiment.units[p]));
            self.units[u].alive() && rate > 0.0 && self.units[u].health < health
        };
        let lost = |&p: &usize| {
            let unit = &self.units[regiment.units[p]];
            !unit.alive() && unit.unpacked.is_none() && rate(regiment.units[p]).0 > 0.0
        };
        // The leader's place comes before the rest, in their order.
        let places: Vec<usize> = (regiment.leader.into_iter())
            .chain((0..regiment.units.len()).filter(|&p| Some(p) != regiment.leader))
            .collect();
        let zone = (self.pathing).zone(self.leader_position(r), regiment.control_range);
        let near = |&p: &usize| {
            Some(p) == regiment.leader || self.in_zone(&zone, Target::Unit(regiment.units[p]))
        };
        if let Some(&p) = places.iter().find(|p| hurt(p) && near(p)) {
            let (u, (rate, health)) = (regiment.units[p], rate(regiment.units[p]));
            let unit = &mut self.units[u];
            unit.health = (unit.health + rate * health).min(health);
        } else if let Some(&p) = places.iter().find(|p| lost(p)) {
            let (rate, health) = rate(regiment.units[p]);
            let built = self.regiments[r].replacing[p] + rate * health;
            self.regiments[r].replacing[p] = built;
            if built >= health {
                self.regiments[r].replacing[p] = 0.0;
                self.arrive(r, p);
            }
        }
    }

    /// Puts the member in place `p` of the regiment at `r` on the map, in
    /// full health: out of a building near the regiment that recruits its
    /// kind, if there is one, else in its place, and on its way to where
    /// the regiment's going.
    fn arrive(&mut self, r: usize, p: usize) {
        let regiment = &self.regiments[r];
        let (at, facing) = (self.leader_position(r), regiment.facing);
        let offsets = &regiment.formations[regiment.current_formation()].offsets;
        let offset = |p: usize| offsets.get(p).copied().unwrap_or([0.0, 0.0]);
        let recruits = |b: &&Building| {
            let list = self.recruit_lists.get(&Id::from(b.template.as_str()));
            b.standing()
                && !b.busy()
                && self.shares_supply(regiment.owner, b.owner)
                && list.is_some_and(|l| l.contains(&Id::from(regiment.template.as_str())))
                && distance(at, b.position) < regiment.control_range
        };
        let nearest = (self.buildings.iter().filter(recruits))
            .min_by(|a, b| distance(at, a.position).total_cmp(&distance(at, b.position)));
        let out =
            nearest.and_then(|b| Some(deploy_point(b.position, b.facing, b.deploy?.creation)));
        // Where it stands from the leader, as in the formation.
        let [lx, ly] = regiment.leader.map_or([0.0, 0.0], offset);
        let [x, y] = offset(p);
        let stand = out.unwrap_or(aaa_map::from_local(at, facing, [x - lx, y - ly]));
        let place = aaa_map::from_local(regiment.position, facing, offset(p));
        let u = regiment.units[p];
        let unit = &mut self.units[u];
        unit.died = None;
        unit.awaited = false;
        unit.health = unit.fighter.as_ref().map_or(0.0, |f| f.health);
        unit.position = stand;
        unit.previous = stand;
        unit.facing = facing;
        unit.speed = 0.0;
        unit.closing = false;
        unit.target = None;
        unit.reloads.fill(0.0);
        if let Some(t) = &mut unit.trailer {
            t.behind(stand, facing);
        }
        self.take_place(u, place, facing, false);
    }

    /// A second of going without for the regiment at `r`: each member
    /// loses about `rate` of its full health, some more and some less.
    fn wear(&mut self, r: usize, rate: f32) {
        let ticks = self.ticks;
        for u in self.regiments[r].units.clone() {
            let (least, most) = ATTRITION;
            let share = least + (most - least) * self.chance();
            let unit = &mut self.units[u];
            let Some(health) = unit
                .fighter
                .as_ref()
                .map(|f| f.health)
                .filter(|_| unit.alive())
            else {
                continue;
            };
            unit.health -= rate * health * share;
            if unit.health <= 0.0 {
                unit.die(ticks);
            }
        }
    }
}
