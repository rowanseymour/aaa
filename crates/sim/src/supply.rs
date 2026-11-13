//! Supply: which buildings give it and over what ground, and how those
//! that only pass it on stay connected to those that give it of
//! themselves.

use std::collections::BTreeMap;

use aaa_map::{Id, Obstacle, Operation, Shape, Supply, SupplyEffect, Tile, Weapon, Zone};

use crate::{
    Building, Enemy, Held, Order, Refused, Relation, State, TICK, Target, World, bounding_radius,
    deploy_point, distance,
};

/// Ticks between looks at whether a building gives supply, each building
/// on its own beat.
pub(crate) const BEAT: u64 = (1.0 / TICK) as u64;

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
    laid: Option<(Tile, f32)>,
}

impl Supplying {
    /// The tile its zone is laid around and how far it reaches, which
    /// between them say what ground it covers.
    pub fn laid(&self) -> Option<(Tile, f32)> {
        self.laid.filter(|_| self.zone.is_some())
    }
}

impl World {
    /// How the building at `b` gives supply, if it does.
    pub fn supply_of(&self, b: usize) -> Option<Supply> {
        self.rules
            .supplies
            .get(&self.buildings[b].template)
            .copied()
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
        template: &Id,
        held: &'a [Held],
    ) -> impl Iterator<Item = &'a SupplyEffect> {
        let kind = self.rules.kinds.get(template);
        let on = move |e: &&SupplyEffect| {
            let has = |p| kind.is_some_and(|k| k.properties.contains(p));
            e.requires.iter().all(has)
        };
        let technologies = self.technologies.get(&owner).into_iter().flatten();
        let timed = held.iter().map(|h| &h.property);
        let of = |p| self.rules.supply_effects.get(p);
        (technologies.filter_map(of).filter(on)).chain(timed.filter_map(of))
    }

    /// Carries out what the special operation `operation` does to supply
    /// for `player`, aimed at `at`. What using one takes is left to
    /// whoever asks.
    pub fn operate(&mut self, player: u32, operation: &Id, at: [f32; 2]) -> Result<(), Refused> {
        match self.rules.operations.get(operation).cloned() {
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
                        let kind = self.rules.kinds.get(&regiment.template);
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
                let mut dropped = self
                    .rules
                    .drops
                    .get(&template)
                    .cloned()
                    .ok_or(Refused::Unable)?;
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
                true => (stockpile + self.rules.supply_rules.recharge).min(supply.stockpile_max),
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
        let around = gives.then(|| (Tile::at(building.position), range));
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
        let (ticks, timeout) = (self.ticks, self.rules.supply_rules.bombard_timeout);
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
        (1.0 + added).max(self.rules.supply_rules.multiply_floor)
    }

    /// The morale a second being in supply adds to what the regiment at
    /// `r` regains while it's doing something that lets it regain any.
    /// Supply does nothing else to morale.
    pub fn morale_bonus(&self, r: usize) -> f32 {
        match self.regiments[r].in_supply {
            true => self.rules.supply_rules.morale_bonus,
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
            let rate = self.rules.resupply_rates.get(&unit.template);
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
        let zone = (self.pathing).zone(
            self.leader_position(r),
            regiment.control(&self.rules.guard_rules),
        );
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
            let list = self.rules.recruit_lists.get(&b.template);
            b.standing()
                && !b.busy()
                && self.shares_supply(regiment.owner, b.owner)
                && list.is_some_and(|l| l.contains(&regiment.template))
                && distance(at, b.position) < regiment.control(&self.rules.guard_rules)
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
        unit.stand_at(stand, facing);
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

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use aaa_map::{DeployPoints, Domain, Id, Pathing, Shape, Weapon};

    use crate::{
        Command, Commission, Enemy, Ledger, Notice, Pause, Refused, Regiment, Relation, State,
        Supplying, Target, World, deploy_point,
    };

    /// Player 1's Corps HQ at 20,20 and two depots east of it, the first
    /// in the HQ's zone and the second only in the first's, each reaching
    /// 20 and the depots stocking up to 3 seconds.
    fn chain() -> World {
        let mut w = World::from_regiments(at_war(), Pathing::uniform(40, Domain::Land), &[]);
        let mut corps = building(5, 1, [20.0, 20.0]);
        corps.template = "corps".into();
        let depot = |id, x| {
            let mut depot = building(id, 1, [x, 20.0]);
            depot.template = "depot".into();
            depot
        };
        w.add_buildings(&[depot(7, 50.0), depot(6, 34.0), corps]);
        w.rules
            .supplies
            .insert(Id::from("corps"), supply(20.0, false, 0.0));
        w.rules
            .supplies
            .insert(Id::from("depot"), supply(20.0, true, 3.0));
        w
    }

    /// [`division`] with a second company at 26,28 (regiment 2), both
    /// infantry, and an operation that resupplies the nearest infantry
    /// regiment within 15 for 10 seconds.
    fn resupplied() -> (World, Id) {
        let mut w = division();
        let mut c = raised_company(&mut w, 1);
        c.renumber(80);
        c.place([26.0, 28.0], [0.0, 1.0]);
        w.add_regiment(&c);
        let kind = aaa_map::Kind {
            properties: Vec::new(),
            flags: vec![Id::from("org_infantry")],
            soft: Shape::Circle { radius: 1.0 },
        };
        w.rules.kinds.insert(Id::from("company"), kind);
        w.rules
            .supply_effects
            .insert(Id::from("resupplied"), effect(0.0, 0.25, true, &[]));
        let operation = aaa_map::Operation::Enchant {
            targets: aaa_map::Targets {
                required: ["FRIEND", "ORGANIZATION", "ORG_INFANTRY"]
                    .map(Id::from)
                    .to_vec(),
                excluded: Vec::new(),
            },
            area: 15.0,
            max_targets: 1,
            property: Id::from("resupplied"),
            seconds: 10.0,
        };
        w.rules.operations.insert(Id::from("resupply"), operation);
        (w, Id::from("resupply"))
    }

    #[test]
    fn relays_pass_supply_on_down_a_chain_from_a_source() {
        let mut w = chain();
        // At once, though the far depot comes first.
        w.tick();
        assert!(w.buildings.iter().all(|b| b.supply.zone.is_some()));
        assert!(w.buildings[0].supply.connected && w.buildings[1].supply.connected);
        assert!(w.supplied(1, [64.0, 20.0]));
        assert!(!w.supplied(1, [20.0, 64.0]));
        // Allies share it, enemies don't.
        assert_eq!(w.sides.relation(1, 3), Relation::Ally);
        assert!(w.supplied(3, [64.0, 20.0]));
        assert!(!w.supplied(2, [64.0, 20.0]));
    }

    #[test]
    fn relays_cut_off_live_on_their_stockpiles_but_pass_nothing_on() {
        let mut w = chain();
        run(&mut w, 5.0);
        assert_eq!(w.buildings[0].supply.stockpile, 3.0);
        w.buildings[2].state = State::Disabled;
        run(&mut w, 1.5);
        assert!(w.buildings[2].supply.zone.is_none());
        // Both still supply what's around them, the far one cut off
        // though the near one's zone still covers it.
        for b in 0..2 {
            let s = &w.buildings[b].supply;
            assert!(!s.connected && s.zone.is_some() && s.stockpile > 0.0);
            assert!(!w.out_of_supply(b));
        }
        run(&mut w, 3.0);
        for b in 0..2 {
            assert!(w.out_of_supply(b) && w.buildings[b].supply.zone.is_none());
        }
        assert!(!w.supplied(1, [64.0, 20.0]));
        w.buildings[2].state = State::Standing;
        run(&mut w, 3.0);
        assert!(w.buildings.iter().all(|b| b.supply.zone.is_some()));
        assert!(!w.out_of_supply(0));
    }

    #[test]
    fn relays_dont_keep_each_other_going() {
        let mut w = chain();
        w.buildings.pop();
        run(&mut w, 2.0);
        assert!(w.out_of_supply(0) && w.out_of_supply(1));
        assert!(!w.supplied(1, [34.0, 20.0]));
    }

    #[test]
    fn captured_relays_connect_on_their_new_side() {
        let mut w = chain();
        w.tick();
        w.change_hands(1, 2);
        w.tick();
        // Player 2 has no source, and player 1's far depot has lost its
        // link by its next look.
        assert!(w.out_of_supply(1));
        run(&mut w, 1.0);
        assert!(!w.buildings[0].supply.connected);
        assert!(!w.supplied(2, [34.0, 20.0]));
    }

    #[test]
    fn cities_supply_only_while_their_side_outweighs_its_enemies_there() {
        let mut centre = city()[0].clone();
        centre.template = "centre".into();
        let mut w = siege(&[centre]);
        w.rules
            .supplies
            .insert(Id::from("centre"), supply(20.0, false, 0.0));
        // Nobody's there.
        run(&mut w, 1.0);
        assert!(w.buildings[0].supply.zone.is_none());
        let to = |w: &mut World, x| {
            let to = [x, 10.0];
            w.apply(1, Command::Move { regiment: 0, to }).unwrap();
            run(w, 15.0);
        };
        to(&mut w, 30.0);
        assert_eq!(w.buildings[0].owner, 1);
        assert!(w.supplied(1, [40.0, 20.0]));
        to(&mut w, 10.0);
        assert!(!w.supplied(1, [40.0, 20.0]));
    }

    #[test]
    fn relays_out_of_supply_earn_as_packed_and_hold_their_queues() {
        let mut w = economy();
        w.rules.packing.insert("depot".into(), packs());
        let mut depot = truck().unpacks.unwrap().building;
        (depot.id, depot.owner, depot.position) = (41, 1, [50.0, 50.0]);
        depot.obstacle.as_mut().unwrap().position = depot.position;
        depot.deploy = w.buildings[0].deploy;
        w.add_buildings(&[depot]);
        w.rules
            .supplies
            .insert(Id::from("depot"), supply(20.0, true, 2.0));
        let room = aaa_map::Economy {
            production: amounts(&[("building_limit_provided", 20.0)]),
            ..Default::default()
        };
        w.rules.economies.insert(Id::from("start_loc"), room);
        w.holdings.push((1, "start_loc".into()));
        let order = Command::Commission {
            building: 1,
            commission: Box::new(Commission::new(truck(), 1.0)),
        };
        w.apply(1, order).unwrap();
        run(&mut w, 3.0);
        // The truck's 12, not the depot's 25, on top of the HQ's 25; its
        // upkeep is the same.
        assert_eq!(w.building_ledger(1).production.get("money"), 12.0);
        assert_eq!(w.balance(1).limits[1].used, 2.0);
        assert_eq!(w.buildings[1].paused, Some(Pause::Supply));
        assert_eq!(w.buildings[1].progress, 0);
        let told = |n: &&Notice| {
            let why = Pause::Supply;
            matches!(n, Notice::Paused { building: 1, why: w, .. } if *w == why)
        };
        assert_eq!(w.notices.iter().filter(told).count(), 1);
        // The HQ's zone reaching it puts it back to work.
        w.rules
            .supplies
            .insert(Id::from("house"), supply(60.0, false, 0.0));
        w.buildings[0].supply = Supplying::default();
        run(&mut w, 3.0);
        assert_eq!(w.building_ledger(1).production.get("money"), 25.0);
        assert!(w.buildings[1].queue.is_empty());
        // What can't pack earns in full without supply unless it needs it.
        w.rules.packing.clear();
        w.rules.supplies.remove(&Id::from("house"));
        run(&mut w, 4.0);
        assert!(w.out_of_supply(1));
        assert_eq!(w.building_ledger(1).production.get("money"), 25.0);
        w.rules
            .economies
            .get_mut(&Id::from("depot"))
            .unwrap()
            .supply_required = true;
        assert_eq!(w.building_ledger(1).production.get("money"), 0.0);
    }

    #[test]
    fn regiments_in_supply_heal_and_fill_their_ranks_one_at_a_time() {
        let mut w = division();
        let [a, b, c] = w.regiments[1].units[..] else {
            panic!("three members");
        };
        w.units[a].health = 20.0;
        w.units[b].health = 20.0;
        w.units[c].die(0);
        // Detached, so out of supply though it's in the HQ's zone.
        run(&mut w, 2.0);
        assert!(!w.regiments[1].in_supply);
        assert_eq!(w.units[a].health, 20.0);
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        // Half its health a second: the leader first, then the next.
        run(&mut w, 1.0);
        assert!(w.regiments[1].in_supply);
        assert_eq!((w.units[a].health, w.units[b].health), (70.0, 20.0));
        run(&mut w, 1.0);
        assert_eq!((w.units[a].health, w.units[b].health), (100.0, 20.0));
        run(&mut w, 2.0);
        assert_eq!(w.units[b].health, 100.0);
        assert!(!w.units[c].alive());
        // Then the one lost, back in its place once built up in full.
        run(&mut w, 1.0);
        assert_eq!(w.regiments[1].replacing[2], 50.0);
        run(&mut w, 1.0);
        assert!(w.units[c].alive());
        assert_eq!(w.units[c].health, 100.0);
        assert_eq!(w.regiments[1].replacing[2], 0.0);
        let place = w.regiments[1].formations[0].offsets[2];
        let place = aaa_map::from_local(w.regiments[1].position, [0.0, 1.0], place);
        run(&mut w, 2.0);
        assert!(near(w.units[c].position, place));
        // Outside every zone it gets nothing.
        w.units[a].health = 20.0;
        for u in [a, b, c] {
            w.units[u].position = [70.0, 70.0];
        }
        run(&mut w, 2.0);
        assert!(!w.regiments[1].in_supply);
        assert_eq!(w.units[a].health, 20.0);
    }

    #[test]
    fn buildings_give_no_supply_but_earn_while_they_unpack() {
        let mut w = division();
        w.rules
            .supplies
            .insert(Id::from("depot"), supply(20.0, true, 30.0));
        let hq = Command::Unpack {
            regiment: 0,
            at: [30.0, 40.0],
            facing: NORTH,
        };
        w.apply(1, hq).unwrap();
        while w.regiments[0].unpacked_into.is_none() {
            w.tick();
        }
        let b = w.regiments[0].unpacked_into.unwrap();
        run(&mut w, 1.0);
        assert!(w.buildings[b].busy());
        assert!(w.buildings[b].supply.zone.is_none());
        // In the house's zone, at its own rate, not its truck's.
        assert_eq!(w.building_ledger(b).production.get("money"), 25.0);
        // Its zone comes the tick it's done, and goes with its stockpile
        // the tick it starts to pack.
        while w.buildings[b].busy() {
            assert!(w.buildings[b].supply.zone.is_none());
            w.tick();
        }
        w.tick();
        assert!(w.buildings[b].supply.zone.is_some());
        run(&mut w, 5.0);
        assert!(w.buildings[b].supply.stockpile > 0.0);
        w.buildings[b].packing = Some((w.ticks, w.ticks + 50));
        w.tick();
        assert!(w.buildings[b].supply.zone.is_none());
        assert_eq!(w.buildings[b].supply.stockpile, 0.0);
    }

    #[test]
    fn replacements_leave_a_building_by_its_deployment_point() {
        let mut w = division();
        // The house recruits its kind, 6 to its left, within its reach.
        w.rules
            .recruit_lists
            .insert(Id::from("house"), vec![Id::from("company")]);
        w.buildings[0].deploy = Some(DeployPoints {
            creation: [0.0, 0.0],
            deployment: [-6.0, 0.0],
        });
        let lost = w.regiments[1].units[2];
        w.units[lost].die(0);
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        while !w.units[lost].alive() {
            w.tick();
        }
        let b = &w.buildings[0];
        let out = deploy_point(b.position, b.facing, [-6.0, 0.0]);
        assert!(near(w.units[lost].position, b.position));
        assert_eq!(w.units[lost].path.last(), Some(&out));
        assert!(w.units[lost].path.len() > 1);
    }

    #[test]
    fn buildings_engage_only_what_their_weapons_target() {
        let mut w = division();
        w.buildings[0].owner = 2;
        w.buildings[0].noticed.push(Enemy::Regiment(1));
        assert!(!w.engaged_by(1));
        let member = w.regiments[1].units[0];
        w.buildings[0].target = Some(Target::Unit(member));
        assert!(w.engaged_by(1));
    }

    #[test]
    fn regiments_a_map_has_short_of_members_fill_up_too() {
        let mut w = division();
        let mut c = raised_company(&mut w, 1);
        c.renumber(80);
        c.members[2].lost = Some(50.0);
        c.place([24.0, 22.0], [0.0, 1.0]);
        let r = w.add_regiment(&c);
        let lost = w.regiments[r].units[2];
        assert!(w.units[lost].awaited && !w.units[lost].alive());
        assert_eq!(w.regiments[r].replacing, [0.0, 0.0, 50.0]);
    }

    #[test]
    fn regiments_keep_up_every_place_whoever_is_lost() {
        let mut w = division();
        let before = w.regiment_ledger(1);
        let last = w.regiments[1].units[2];
        w.units[last].die(0);
        assert_eq!(w.regiment_ledger(1), before);
        w.destroy(1);
        assert_eq!(w.regiment_ledger(1), Ledger::default());
    }

    #[test]
    fn regiments_out_of_supply_wear_away_while_their_side_is_short() {
        let mut w = division();
        // Costs it can't meet, and no money.
        w.rules
            .economies
            .get_mut(&Id::from("house"))
            .unwrap()
            .upkeep = amounts(&[("money", 500.0)]);
        w.set_stockpile(1, 0.0);
        run(&mut w, 3.0);
        assert!(w.short(1).is_some());
        let a = w.regiments[1].units[0];
        let lost = 100.0 - w.units[a].health;
        // Two or three looks, each of 0.75 to 1.75 of a hundredth.
        assert!((1.5..=5.25).contains(&lost), "{lost}");
        // The HQ's truck, which uses no supply, is untouched.
        let truck = w.regiments[0].units[0];
        assert_eq!(w.units[truck].health, 100.0);
        // In supply it's spared, and resupplies the slower for the
        // shortage.
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        w.units[a].health = 10.0;
        run(&mut w, 1.0);
        let gained = w.units[a].health - 10.0;
        assert!(gained > 0.0 && gained < 50.0, "{gained}");
        // It kills in the end.
        w.apply(1, Command::Detach { regiment: 1 }).unwrap();
        run(&mut w, 200.0);
        assert!(w.destroyed(1));
    }

    #[test]
    fn hqs_raise_what_they_have_places_for_as_leaders_alone() {
        let mut w = division();
        let hq = Command::Unpack {
            regiment: 0,
            at: [30.0, 40.0],
            facing: NORTH,
        };
        w.apply(1, hq).unwrap();
        run(&mut w, 30.0);
        let b = w.regiments[0].unpacked_into.expect("unpacked");
        assert!(!w.buildings[b].busy());
        assert_eq!(w.hq_of(b), Some(50));
        w.buildings[b].deploy = w.buildings[0].deploy;
        let mut company = raised_company(&mut w, 1);
        company.primary = Some(1);
        for m in &mut company.members {
            m.template = "soldier".into();
            m.fighter = Some(soldier(Vec::new()));
        }
        let order = |c: &aaa_map::Regiment| Command::Commission {
            building: b,
            commission: Box::new(Commission::new(c.clone(), 1.0)),
        };
        // One place is the detached company's to take; the other's held
        // from the order on.
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        w.apply(1, order(&company)).unwrap();
        assert_eq!(w.check(1, &order(&company)), Err(Refused::Full));
        let paid = w.stockpile(1);
        assert_eq!(w.apply(1, order(&company)), Err(Refused::Full));
        assert_eq!(w.stockpile(1), paid);
        run(&mut w, 1.5);
        let r = w.regiments.len() - 1;
        assert_eq!(w.regiments[r].hq, Some(50));
        let alive: Vec<bool> = (w.regiments[r].units.iter())
            .map(|&u| w.units[u].alive())
            .collect();
        assert_eq!(alive, [false, true, false]);
        // It costs its whole upkeep from the start, and fills up in supply.
        assert_eq!(w.regiment_ledger(r), w.regiment_ledger(1));
        run(&mut w, 6.0);
        assert!(w.regiments[r].units.iter().all(|&u| w.units[u].alive()));
    }

    #[test]
    fn bombardment_keeps_regiments_out_of_supply_a_while() {
        let mut w = supplied_division();
        w.add_buildings(&[building(30, 2, [60.0, 60.0])]);
        let gun = Target::Building(1);
        // A second from the hit, then until its next look.
        w.shell(1, gun, 4.0);
        run(&mut w, 1.0);
        assert!(!w.regiments[1].in_supply);
        run(&mut w, 1.0);
        assert!(w.regiments[1].in_supply && w.regiments[1].bombarded.is_empty());
        // One that's bombarding waits out the weapon's recharge too, from
        // each hit, and doesn't resupply meanwhile.
        let a = w.regiments[1].units[0];
        w.units[a].health = 10.0;
        w.regiments[1].bombarding = true;
        w.shell(1, gun, 4.0);
        run(&mut w, 3.0);
        w.regiments[1].bombarding = false;
        w.shell(1, gun, 4.0);
        assert_eq!(w.regiments[1].bombarded[0].length, 50);
        run(&mut w, 4.0);
        assert!(!w.regiments[1].in_supply);
        run(&mut w, 2.0);
        assert!(w.regiments[1].in_supply);
        assert_eq!(w.units[a].health, 60.0);
        // What bombarded it is forgotten once it's gone.
        w.shell(1, gun, 4.0);
        w.buildings[1].state = State::Destroyed(w.ticks);
        run(&mut w, 1.0);
        assert!(w.regiments[1].in_supply);
    }

    #[test]
    fn only_bombardment_hits_count_and_they_draw_no_fire() {
        let shelled = |bombard: bool| {
            let mut w = supplied_division();
            let mut gun = building(30, 2, [40.0, 14.0]);
            gun.fighter.notice = Some(30.0);
            gun.fighter.weapons = vec![Weapon {
                bombard,
                ..rifle(30.0, 10.0)
            }];
            w.add_buildings(&[gun]);
            w.regiments[1].operational_range = 34.0;
            w.tick();
            let hurt = |u: &&usize| w.units[**u].health == 90.0;
            assert_eq!(w.regiments[1].units.iter().filter(hurt).count(), 1);
            (
                w.regiments[1].bombarded.clone(),
                w.regiments[1].engaged.clone(),
            )
        };
        let (bombarded, engaged) = shelled(true);
        assert_eq!(bombarded.len(), 1);
        assert_eq!(bombarded[0].by, Enemy::Building(1));
        assert!(engaged.is_empty());
        let (bombarded, engaged) = shelled(false);
        assert!(bombarded.is_empty());
        assert_eq!(engaged, [Enemy::Building(1)]);
        // A building that picks its own shots makes ordinary hits, and a
        // regiment ordered to bombard does so with whatever it fires.
        let mut w = supplied_division();
        let mut gun = building(30, 2, [40.0, 14.0]);
        gun.fighter.weapons = vec![Weapon {
            bombard: true,
            ..rifle(30.0, 10.0)
        }];
        w.add_buildings(&[gun]);
        w.tick();
        assert!(w.regiments[1].bombarded.is_empty());
        let mut w = battle(&[1, 2], vec![soldier(vec![rifle(12.0, 30.0)]); 2], at_war());
        w.regiments[0].bombarding = true;
        w.tick();
        assert_eq!(w.regiments[1].bombarded[0].by, Enemy::Regiment(0));
        assert!(w.regiments[0].bombarded.is_empty());
    }

    #[test]
    fn technologies_and_properties_change_supply_ranges() {
        let mut w = supplied_division();
        w.rules
            .supply_effects
            .insert(Id::from("logistics"), effect(5.0, 0.0, false, &[]));
        w.rules
            .supply_effects
            .insert(Id::from("sabotage"), effect(-100.0, 0.0, false, &[]));
        // 30 reaches 14 tiles straight out and 35 reaches 17.
        let far = [20.0 + 2.0 * 16.0, 20.0];
        assert_eq!(w.supply_range(0), Some(30.0));
        assert!(!w.supplied(1, far));
        w.grant(1, Id::from("logistics"));
        w.grant(2, Id::from("sabotage"));
        run(&mut w, 1.0);
        assert_eq!(w.supply_range(0), Some(35.0));
        assert!(w.supplied(1, far));
        // Below nothing it gives none, for as long as that lasts.
        w.give(Enemy::Building(0), Id::from("sabotage"), 3.0);
        run(&mut w, 1.0);
        assert_eq!(w.supply_range(0), Some(-65.0));
        assert!(w.buildings[0].supply.zone.is_none());
        run(&mut w, 1.0);
        assert!(!w.regiments[1].in_supply);
        run(&mut w, 2.0);
        assert!(w.supplied(1, far) && w.buildings[0].properties.is_empty());
    }

    #[test]
    fn technologies_and_properties_change_how_fast_regiments_resupply() {
        let mut w = supplied_division();
        w.rules.supply_effects.insert(
            Id::from("foraging"),
            effect(0.0, 0.25, false, &["org_unit"]),
        );
        w.rules
            .supply_effects
            .insert(Id::from("rations"), effect(0.0, 0.25, false, &[]));
        w.rules
            .supply_effects
            .insert(Id::from("famine"), effect(0.0, -3.0, false, &[]));
        // Only on those with the properties a technology needs.
        w.grant(1, Id::from("foraging"));
        assert_eq!(w.resupply_multiplier(1), 1.0);
        let kind = aaa_map::Kind {
            properties: vec![Id::from("org_unit")],
            ..Default::default()
        };
        w.rules.kinds.insert(Id::from("company"), kind);
        assert_eq!(w.resupply_multiplier(1), 1.25);
        // They add up, on the half its health a second it gets back.
        w.give(Enemy::Regiment(1), Id::from("rations"), 1.0);
        assert_eq!(w.resupply_multiplier(1), 1.5);
        let a = w.regiments[1].units[0];
        w.units[a].health = 10.0;
        run(&mut w, 1.0);
        assert_eq!(w.units[a].health, 85.0);
        // A property goes at its holder's first look after its time's up.
        assert_eq!(w.resupply_multiplier(1), 1.5);
        run(&mut w, 1.0);
        assert_eq!(w.resupply_multiplier(1), 1.25);
        // But never to less than the least a multiplier can be.
        w.give(Enemy::Regiment(1), Id::from("famine"), 5.0);
        assert_eq!(w.resupply_multiplier(1), 0.05);
    }

    #[test]
    fn resupply_operations_work_on_the_nearest_friends_of_their_class() {
        let (mut w, resupply) = resupplied();
        let given = |w: &World| -> Vec<bool> {
            let has = |r: &Regiment| !r.properties.is_empty();
            w.regiments.iter().map(has).collect()
        };
        assert_eq!(
            w.operate(1, &Id::from("retreat"), [26.0, 21.0]),
            Err(Refused::Unable)
        );
        // Not on what's out of reach, to the edge of its soft shape, nor
        // for an enemy.
        w.operate(1, &resupply, [26.0, 44.5]).unwrap();
        w.operate(2, &resupply, [26.0, 21.0]).unwrap();
        assert_eq!(given(&w), [false, false, false]);
        // The HQ's truck is nearest but no infantry, and it only works on
        // so many. An ally's works as one's own.
        w.operate(3, &resupply, [26.0, 44.0]).unwrap();
        assert_eq!(given(&w), [false, false, true]);
        // One that still has it gets no more of it, but takes a place.
        run(&mut w, 1.0);
        let held = w.regiments[2].properties.clone();
        w.operate(1, &resupply, [29.0, 28.0]).unwrap();
        assert_eq!(given(&w), [false, false, true]);
        assert_eq!(w.regiments[2].properties, held);
        w.operate(1, &resupply, [26.0, 21.0]).unwrap();
        assert_eq!(given(&w), [false, true, true]);
    }

    #[test]
    fn resupply_operations_stand_in_for_an_hq_and_a_zone_for_a_while() {
        let (mut w, resupply) = resupplied();
        let a = w.regiments[1].units[0];
        w.units[a].health = 10.0;
        run(&mut w, 1.0);
        assert!(!w.regiments[1].in_supply);
        // Detached, it resupplies, and a quarter faster.
        w.operate(1, &resupply, [26.0, 21.0]).unwrap();
        run(&mut w, 1.0);
        assert!(w.regiments[1].in_supply && w.regiments[1].hq.is_none());
        assert_eq!(w.units[a].health, 72.5);
        // Outside every zone too, unless its side can't pay its upkeep.
        for u in w.regiments[1].units.clone() {
            w.units[u].position = [70.0, 70.0];
        }
        run(&mut w, 1.0);
        assert!(w.regiments[1].in_supply);
        w.rules
            .economies
            .get_mut(&Id::from("house"))
            .unwrap()
            .upkeep = amounts(&[("money", 500.0)]);
        w.set_stockpile(1, 0.0);
        run(&mut w, 2.0);
        assert!(w.short(1).is_some() && !w.regiments[1].in_supply);
        // Once it's run out the regiment's as it was.
        w.rules
            .economies
            .get_mut(&Id::from("house"))
            .unwrap()
            .upkeep = amounts(&[]);
        for u in w.regiments[1].units.clone() {
            w.units[u].position = [26.0, 20.0];
        }
        run(&mut w, 2.0);
        assert!(w.regiments[1].in_supply);
        run(&mut w, 5.0);
        assert!(!w.regiments[1].in_supply && w.regiments[1].properties.is_empty());
    }

    #[test]
    fn supply_drops_supply_their_side_until_they_decay() {
        let mut w = supplied_division();
        let mut provisions = building(0, 0, [0.0, 0.0]);
        provisions.template = "provisions".into();
        provisions.fighter.destructible = true;
        w.rules.drops.insert(Id::from("provisions"), provisions);
        w.rules
            .supplies
            .insert(Id::from("provisions"), supply(10.0, false, 0.0));
        w.rules.decays.insert(Id::from("provisions"), 10.0);
        let drop = Id::from("drop");
        let dropped = aaa_map::Operation::Drop(Id::from("provisions"));
        w.rules.operations.insert(drop.clone(), dropped);
        w.rules.operations.insert(
            Id::from("airlift"),
            aaa_map::Operation::Drop(Id::from("tank")),
        );
        assert_eq!(
            w.operate(1, &Id::from("airlift"), [64.0, 64.0]),
            Err(Refused::Unable)
        );
        assert_eq!(w.operate(1, &drop, [64.0, 90.0]), Err(Refused::NoRoom));
        // A source of its side's and its allies' from its first look.
        w.operate(1, &drop, [64.0, 64.0]).unwrap();
        let b = w.buildings.len() - 1;
        assert_eq!(w.buildings[b].owner, 1);
        assert!(w.buildings[b].supply.zone.is_none());
        run(&mut w, 1.0);
        assert!(w.supplied(1, [64.0, 64.0]) && w.supplied(3, [64.0, 64.0]));
        assert!(!w.supplied(2, [64.0, 64.0]));
        for u in w.regiments[1].units.clone() {
            w.units[u].position = [62.0, 64.0];
        }
        run(&mut w, 1.0);
        assert!(w.regiments[1].in_supply);
        // Its zone follows it, as it does on the way down.
        assert!(!w.supplied(1, [44.0, 64.0]));
        w.buildings[b].position = [50.0, 64.0];
        run(&mut w, 1.0);
        assert!(w.supplied(1, [44.0, 64.0]) && !w.supplied(1, [64.0, 64.0]));
        // It loses 10 of its 100 a second from when it was dropped, then
        // goes, and its zone with it.
        run(&mut w, 7.0);
        assert_eq!(w.buildings[b].health, 10.0);
        w.tick();
        assert!(matches!(w.buildings[b].state, State::Destroyed(_)));
        run(&mut w, 1.0);
        assert!(!w.supplied(1, [44.0, 64.0]));
    }

    #[test]
    fn regiments_in_supply_regain_more_morale() {
        let mut w = division();
        run(&mut w, 1.0);
        assert_eq!(w.morale_bonus(1), 0.0);
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        run(&mut w, 1.0);
        assert_eq!(w.morale_bonus(1), w.rules.supply_rules.morale_bonus);
        assert!(w.morale_bonus(1) > 0.0);
    }
}
