//! Supply: which buildings give it and over what ground, and how those
//! that only pass it on stay connected to those that give it of
//! themselves.

use std::collections::BTreeMap;

use aaa_map::pathing::tile_at;
use aaa_map::{Id, Obstacle, Operation, Shape, Supply, SupplyEffect, Weapon, Zone};

use crate::{
    Building, Enemy, Order, Refused, Relation, State, TICK, Target, World, bounding_radius,
    deploy_point, distance,
};

/// Ticks between looks at whether a building gives supply, each building
/// on its own beat.
pub(crate) const BEAT: u64 = (1.0 / TICK) as u64;

/// A property something has for a time.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub property: Id,
    /// The tick its time is up on. It's taken off at its holder's next
    /// look after.
    pub until: u64,
}

/// Something that has bombarded a regiment lately.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bombarder {
    /// The regiment or building that did.
    pub by: Enemy,
    /// The tick its last hit landed on.
    pub tick: u64,
    /// Ticks from then that it keeps the regiment out of supply.
    pub length: u64,
}

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
    /// Whether it was unpacking or packing when last looked at.
    busy: bool,
    /// The tile its zone was laid around, and how far.
    laid: Option<((i32, i32), f32)>,
}

impl Supplying {
    /// The tile its zone is laid around and how far it reaches, which
    /// between them say what ground it covers.
    pub fn laid(&self) -> Option<((i32, i32), f32)> {
        self.laid.filter(|_| self.zone.is_some())
    }
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

    /// How far the supply of the building at `b` reaches, if it gives any:
    /// its own range and what its side's technologies and its properties
    /// add to it.
    pub fn supply_range(&self, b: usize) -> Option<f32> {
        let building = &self.buildings[b];
        let effects = self.effects(building.owner, &building.template, &building.properties);
        Some(self.supply_of(b)?.range + effects.map(|e| e.range).sum::<f32>())
    }

    /// What bears on the supply of something of `owner`'s made from
    /// `template`: its side's technologies that are on its kind, and the
    /// properties it has for a time.
    fn effects<'a>(
        &'a self,
        owner: u32,
        template: &str,
        held: &'a [Held],
    ) -> impl Iterator<Item = &'a SupplyEffect> {
        let kind = self.kinds.get(&Id::from(template));
        let on = move |e: &&SupplyEffect| {
            let has = |p| kind.is_some_and(|k| k.properties.contains(p));
            e.requires.iter().all(has)
        };
        let technologies = self.technologies.get(&owner).into_iter().flatten();
        let timed = held.iter().map(|h| &h.property);
        let of = |p| self.supply_effects.get(p);
        (technologies.filter_map(of).filter(on)).chain(timed.filter_map(of))
    }

    /// Gives `player` a technology, for all it has and will have, unless
    /// the map bars it.
    pub fn grant(&mut self, player: u32, technology: Id) {
        if self.barred(player, &technology) {
            return;
        }
        let held = self.technologies.entry(player).or_default();
        if !held.contains(&technology) {
            held.push(technology);
        }
    }

    /// Gives a regiment or building a property for `seconds` from now,
    /// unless it still has it.
    pub fn give(&mut self, to: Enemy, property: Id, seconds: f32) {
        let until = self.ticks + (seconds / TICK).round() as u64;
        let held = match to {
            Enemy::Regiment(r) => &mut self.regiments[r].properties,
            Enemy::Building(b) => &mut self.buildings[b].properties,
        };
        if !held.iter().any(|h| h.property == property) {
            held.push(Held { property, until });
        }
    }

    /// Carries out what the special operation `operation` does to supply
    /// for `player`, aimed at `at`. What using one takes is left to
    /// whoever asks.
    pub fn operate(&mut self, player: u32, operation: &Id, at: [f32; 2]) -> Result<(), Refused> {
        match self.operations.get(operation).cloned() {
            Some(Operation::Enchant {
                targets,
                area,
                max_targets,
                property,
                seconds,
            }) => {
                let organization = Id::from("organization");
                let mut near: Vec<(f32, usize)> = (0..self.regiments.len())
                    .filter(|&r| !self.destroyed(r))
                    .filter_map(|r| {
                        let regiment = &self.regiments[r];
                        let kind = self.kinds.get(&Id::from(regiment.template.as_str()));
                        let side = self.standing_flag(player, regiment.owner);
                        let flags = kind.into_iter().flat_map(|k| &k.flags);
                        let d = distance(at, self.leader_position(r));
                        let reach = area + kind.map_or(0.0, |k| bounding_radius(k.soft));
                        let fits = targets.allow(flags.chain([&organization, &side]));
                        (d <= reach && fits).then_some((d, r))
                    })
                    .collect();
                near.sort_by(|a, b| a.0.total_cmp(&b.0));
                if max_targets > 0 {
                    near.truncate(max_targets as usize);
                }
                // One that still has it gets nothing, but takes its place.
                for (_, r) in near {
                    self.give(Enemy::Regiment(r), property.clone(), seconds);
                }
            }
            Some(Operation::Drop(template)) => {
                let mut dropped = self.drops.get(&template).cloned().ok_or(Refused::Unable)?;
                let spot = Obstacle {
                    position: at,
                    facing: dropped.facing,
                    shape: Shape::Point,
                    soft: Shape::Point,
                };
                if !self.pathing.on_map(&spot) {
                    return Err(Refused::NoRoom);
                }
                dropped.id = self.take_ids(1);
                dropped.owner = player;
                dropped.position = at;
                // In the air it's in nothing's way.
                dropped.obstacle = None;
                let b = self.buildings.len();
                self.add_buildings(&[dropped]);
                // Its zone waits for its own beat.
                self.buildings[b].supply.judged = true;
            }
            None => return Err(Refused::Unable),
        }
        Ok(())
    }

    /// Whether `a`'s zones supply `b`'s regiments too.
    pub fn shares_supply(&self, a: u32, b: u32) -> bool {
        a == b || self.sides.relation(a, b) == Relation::Ally
    }

    /// Looks at each building's supply: those just put down, taken, or
    /// starting or done packing or unpacking at once, the rest on their
    /// beats.
    pub(crate) fn supply(&mut self) {
        let all = 0..self.buildings.len();
        // And those that have started packing or finished unpacking, whose
        // zones go and come that moment.
        let fresh: Vec<usize> = (all.clone())
            .filter(|&b| {
                let building = &self.buildings[b];
                !building.supply.judged || building.supply.busy != building.busy()
            })
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
            if !(self.ticks + b as u64).is_multiple_of(BEAT) {
                continue;
            }
            let ticks = self.ticks;
            self.buildings[b].properties.retain(|h| h.until > ticks);
            if !fresh.contains(&b) {
                self.judge(b, true);
            }
        }
    }

    /// Works out afresh whether the building at `b` gives supply, laying
    /// or lifting its zone to match, and on its beat filling or draining a
    /// relay's stockpile. Returns whether its zone came, went or changed.
    fn judge(&mut self, b: usize, beat: bool) -> bool {
        let (Some(supply), Some(range)) = (self.supply_of(b), self.supply_range(b)) else {
            return false;
        };
        let building = &self.buildings[b];
        // Unpacking or packing, it gives nothing and holds nothing, and
        // is in supply by the ground it stands on, as a regiment is.
        let busy = building.busy();
        let connected = match busy {
            true => self.covered(b),
            false => !supply.relay || self.reached(b, &mut Vec::new()),
        };
        let mut stockpile = match busy {
            true => 0.0,
            false => building.supply.stockpile,
        };
        if supply.relay && beat && !busy {
            stockpile = match connected {
                true => (stockpile + self.supply_rules.recharge).min(supply.stockpile_max),
                false => stockpile - 1.0,
            }
            .max(0.0);
        }
        let sided = (self.sides.player(building.owner)).is_some_and(|p| !p.independent);
        let gives = sided
            && !busy
            && building.state == State::Standing
            && range >= 0.0
            && (connected || stockpile > 0.0)
            && building.zone.is_none_or(|city| self.held_firmly(b, city));
        // Laid again when its range changes or it moves, as one dropped
        // from the air does on its way down.
        let around = gives.then(|| (tile_at(building.position), range));
        let moved = around != building.supply.laid;
        let laid = (gives && moved).then(|| self.pathing.zone(building.position, range));
        let s = &mut self.buildings[b].supply;
        s.connected = connected;
        s.stockpile = stockpile;
        s.busy = busy;
        s.laid = around;
        match gives {
            true => s.zone = laid.or(s.zone.take()),
            false => s.zone = None,
        }
        moved
    }

    /// Whether a zone of another building of its side or an ally's covers
    /// the building at `b`, wherever that zone's supply comes from.
    fn covered(&self, b: usize) -> bool {
        let owner = self.buildings[b].owner;
        (0..self.buildings.len()).any(|p| {
            let other = &self.buildings[p];
            let zone = other.supply.zone.as_ref();
            p != b
                && self.shares_supply(owner, other.owner)
                && zone.is_some_and(|z| self.in_zone(z, Target::Building(b)))
        })
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

    /// Whether the regiment at `r` is one that needs supply and, as of its
    /// last look, is going without.
    pub fn lacks_supply(&self, r: usize) -> bool {
        let used = self.logistics_of(&self.regiments[r].template);
        used.is_some_and(|l| l.supply_used) && !self.destroyed(r) && !self.regiments[r].in_supply
    }

    /// Whether anything has the regiment at `r` as a target of its
    /// engagement.
    pub(crate) fn engaged_by(&self, r: usize) -> bool {
        let them = Enemy::Regiment(r);
        (0..self.regiments.len())
            .any(|e| self.regiments[e].engaged.contains(&them) && !self.destroyed(e))
            || (self.buildings.iter()).any(|b| {
                // A building counts only while it has one of them as its
                // target, not for having noticed them.
                let aimed =
                    matches!(b.target, Some(Target::Unit(u)) if self.units[u].regiment == r);
                b.standing() && aimed
            })
    }

    /// Whether a shot `s` fires with `weapon` is bombardment. For what
    /// picks its own shots the order decides, whatever the weapon; a
    /// building defending itself doesn't, so there the weapon does.
    pub(crate) fn bombards(&self, s: Target, weapon: &Weapon) -> bool {
        match s {
            Target::Unit(u) => self.regiments[self.units[u].regiment].bombarding,
            Target::Building(b) => weapon.bombard && self.buildings[b].fighter.notice.is_some(),
        }
    }

    /// Notes a bombardment's hit on the regiment at `r` by `from`, whose
    /// weapon takes `recharge` seconds between shots.
    pub(crate) fn shell(&mut self, r: usize, from: Target, recharge: f32) {
        let by = self.enemy(from);
        let (ticks, timeout) = (self.ticks, self.supply_rules.bombard_timeout);
        let regiment = &mut self.regiments[r];
        match regiment.bombarded.iter_mut().find(|b| b.by == by) {
            Some(known) => known.tick = ticks,
            None => {
                // One that's bombarding is kept out until the weapon could
                // have fired again too.
                let seconds = match regiment.bombarding {
                    true => timeout + recharge,
                    false => timeout,
                };
                regiment.bombarded.push(Bombarder {
                    by,
                    tick: ticks,
                    length: (seconds / TICK).round() as u64,
                });
            }
        }
    }

    /// Drops what bombarded the regiment at `r` long enough ago, or is
    /// gone.
    fn forget_bombarders(&mut self, r: usize) {
        let ticks = self.ticks;
        let mut bombarded = std::mem::take(&mut self.regiments[r].bombarded);
        bombarded.retain(|b| {
            let gone = match b.by {
                Enemy::Regiment(e) => self.destroyed(e),
                Enemy::Building(e) => self.buildings[e].gone(),
            };
            ticks < b.tick + b.length && !gone
        });
        self.regiments[r].bombarded = bombarded;
    }

    /// What the rates the regiment at `r` resupplies at are multiplied by:
    /// 1 and what its side's technologies and its properties add.
    pub fn resupply_multiplier(&self, r: usize) -> f32 {
        let regiment = &self.regiments[r];
        let effects = self.effects(regiment.owner, &regiment.template, &regiment.properties);
        let added: f32 = effects.map(|e| e.resupply).sum();
        (1.0 + added).max(self.supply_rules.multiply_floor)
    }

    /// The morale a second being in supply adds to what the regiment at
    /// `r` regains while it's doing something that lets it regain any.
    /// Supply does nothing else to morale.
    pub fn morale_bonus(&self, r: usize) -> f32 {
        match self.regiments[r].in_supply {
            true => self.supply_rules.morale_bonus,
            false => 0.0,
        }
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
            let ticks = self.ticks;
            self.regiments[r].properties.retain(|h| h.until > ticks);
            self.forget_bombarders(r);
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
            let short = *shorts.entry(owner).or_insert_with(|| self.short(owner));
            // Some properties stand in for an HQ, and for a zone while the
            // side can pay its upkeep.
            let flagged =
                (self.effects(owner, &regiment.template, &regiment.properties)).any(|e| e.supplied);
            let in_supply = sided
                && !self.engaged_by(r)
                && regiment.bombarded.is_empty()
                && (regiment.hq.is_some() || logistics.detached || flagged)
                && (flagged && short.is_none() || self.supplied(owner, self.leader_position(r)));
            let resting = regiment.engaged.is_empty()
                && !regiment.bombarding
                && !matches!(regiment.order, Order::Pursue(_));
            self.regiments[r].in_supply = in_supply;
            match (in_supply, short) {
                (true, _) if resting => {
                    let pace = (1.0 - short.unwrap_or(0.0)) * self.resupply_multiplier(r);
                    self.replenish(r, pace);
                }
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
            !unit.alive() && unit.left.is_none() && rate(regiment.units[p]).0 > 0.0
        };
        // The leader's place comes before the rest, in their order.
        let places: Vec<usize> = (regiment.leader.into_iter())
            .chain((0..regiment.units.len()).filter(|&p| Some(p) != regiment.leader))
            .collect();
        let zone =
            (self.pathing).zone(self.leader_position(r), regiment.control(&self.guard_rules));
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
    /// kind, by the way its recruits leave it, if there is one, else in
    /// its place, and on its way to where the regiment's going.
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
                && distance(at, b.position) < regiment.control(&self.guard_rules)
        };
        let nearest = (self.buildings.iter().filter(recruits))
            .min_by(|a, b| distance(at, a.position).total_cmp(&distance(at, b.position)));
        // Out of a building it comes as a recruit does: made at one point
        // and by way of another.
        let points = nearest.and_then(|b| {
            let point = |offset| deploy_point(b.position, b.facing, offset);
            Some((point(b.deploy?.creation), point(b.deploy?.deployment)))
        });
        let out = points.map(|(creation, _)| creation);
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
        match points {
            Some((creation, deployment)) => {
                // Straight out to the deployment point, then by a way
                // found from there.
                self.units[u].position = deployment;
                let mut way = self.plan(u, place, false);
                way.push(deployment);
                let unit = &mut self.units[u];
                unit.position = creation;
                unit.take(way, false);
                unit.end_facing = facing;
            }
            None => self.take_place(u, place, facing, false),
        }
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
                self.lost_member(u);
            }
        }
    }
}
