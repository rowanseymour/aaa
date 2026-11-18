//! Organizations' morale: what wears it down and builds it back up, and
//! routing when it breaks.

use std::f32::consts::TAU;

use tgmap::{Id, MoraleEffect};

use crate::path;
use crate::supply::BEAT;
use crate::{Notice, Order, Piece, Refused, TICK, Target, World, distance, normalised};

/// Seconds a routing organization with nowhere to run waits before it
/// looks again.
const ROUT_RETRY: f32 = 2.0;

/// What an organization's morale has it doing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Mind {
    /// As its orders have it.
    #[default]
    Steady,
    /// Running from the fight, to `to` once it's found somewhere, until
    /// the tick `until`. With nowhere yet, it looks again on the tick
    /// `retry`.
    Routing {
        to: Option<[f32; 2]>,
        until: u64,
        retry: u64,
    },
    /// Got where it ran to, and pulling itself together until the tick
    /// `until`: it does no damage meanwhile.
    Reforming { until: u64 },
    /// Out of morale: it stands until its morale is back above its
    /// breakpoint.
    Exhausted,
}

impl Mind {
    /// The property it gives its organization, as the data names it.
    pub fn property(&self) -> Option<&'static str> {
        match self {
            Mind::Steady => None,
            Mind::Routing { .. } => Some(tgmap::ROUTING),
            Mind::Reforming { .. } => Some(tgmap::REFORMING),
            Mind::Exhausted => Some(tgmap::EXHAUSTED),
        }
    }
}

impl crate::Organization {
    pub fn routing(&self) -> bool {
        matches!(self.mind, Mind::Routing { .. })
    }

    /// Whether it does no damage, as while reforming.
    pub fn harmless(&self) -> bool {
        matches!(self.mind, Mind::Reforming { .. })
    }
}

impl World {
    /// What bears on the morale of the organization at `r`: the properties
    /// of its template and those it has for a time, and its side's
    /// technologies that are on its kind.
    fn morale_effects(&self, r: usize) -> impl Iterator<Item = &MoraleEffect> {
        let organization = &self.organizations[r];
        let (owner, template) = (organization.owner, &organization.template);
        self.template_effects(owner, template, &organization.properties)
    }

    /// What a thing of `owner`'s made from `template` holding `held` has for
    /// morale: its template's own properties, then what
    /// [`World::bearing`] gives.
    fn template_effects<'a>(
        &'a self,
        owner: u32,
        template: &Id,
        held: &'a [crate::Held],
    ) -> impl Iterator<Item = &'a MoraleEffect> {
        let table = &self.rules.morale_effects;
        let kind = self.rules.kinds.get(template);
        let own = kind.into_iter().flat_map(|k| &k.properties);
        let bearing = self.bearing(table, |e| &e.requires, owner, template, held);
        own.filter_map(|p| table.get(p)).chain(bearing)
    }

    /// The most morale the organization at `r` can have: what every slot
    /// of it adds, filled or not, and the `MORALE` its present members and
    /// it have.
    pub(crate) fn maximum_morale(&self, r: usize) -> f32 {
        let organization = &self.organizations[r];
        let value = |u: usize| {
            let template = &self.units[u].template;
            self.rules
                .morale_values
                .get(template)
                .copied()
                .unwrap_or(0.0)
        };
        let slots: f32 = organization.units.iter().map(|&u| value(u)).sum();
        if slots <= 0.0 {
            return 0.0;
        }
        let members: f32 = (self.survivors(r))
            .flat_map(|u| self.template_effects(organization.owner, &self.units[u].template, &[]))
            .map(|e| e.morale)
            .sum();
        let own: f32 = self.morale_effects(r).map(|e| e.morale).sum();
        (slots + members + own).max(0.0)
    }

    /// The morale at or below which the organization at `r` routs.
    pub fn breakpoint(&self, r: usize) -> f32 {
        let template = &self.organizations[r].template;
        let own = self.rules.morale_breakpoints.get(template).copied();
        own.unwrap_or(0.0) + self.morale_effects(r).map(|e| e.breakpoint).sum::<f32>()
    }

    /// Works the organization at `r`'s maximum morale out again, its morale
    /// keeping the same share of it, or coming back to full if `refresh`.
    pub(crate) fn refigure_morale(&mut self, r: usize, refresh: bool) {
        let max = self.maximum_morale(r);
        let organization = &mut self.organizations[r];
        let share = match organization.morale_max > 0.0 && !refresh {
            true => organization.morale / organization.morale_max,
            false => 1.0,
        };
        organization.morale_max = max;
        organization.morale = max * share;
    }

    fn berserk(&self, r: usize) -> bool {
        self.morale_effects(r).any(|e| e.berserk)
    }

    /// Whether an enemy organization is engaging the one at `r`.
    pub(crate) fn engaged_by_enemy(&self, r: usize) -> bool {
        self.engagers(r).next().is_some()
    }

    /// The enemy organizations engaging the one at `r`.
    fn engagers(&self, r: usize) -> impl Iterator<Item = usize> + '_ {
        let them = Piece::Organization(r);
        (0..self.organizations.len()).filter(move |&e| {
            self.organizations[e].engaged.contains(&them)
                && !self.destroyed(e)
                && self.hostile(e, them)
        })
    }

    /// Whether the organization at `r` is in a fight: engaging something,
    /// or engaged by an enemy.
    pub fn fighting(&self, r: usize) -> bool {
        !self.organizations[r].engaged.is_empty() || self.engaged_by_enemy(r)
    }

    /// Takes `amount` of the organization at `r`'s morale for something
    /// that a [`MoraleEffect::berserk`] one shrugs off when `shrugged`, as
    /// fighting, running and weapons are. One unpacked loses none.
    pub(crate) fn lose_morale(&mut self, r: usize, amount: f32, shrugged: bool) {
        let organization = &self.organizations[r];
        if amount <= 0.0
            || organization.morale_max <= 0.0
            || organization.unpacked_into.is_some()
            || shrugged && self.berserk(r)
        {
            return;
        }
        // One with nobody left has no one to break.
        let gone = self.destroyed(r);
        let was = organization.morale;
        let now = (was - amount).max(0.0);
        self.organizations[r].morale = now;
        if gone || self.organizations[r].routing() {
            return;
        }
        let breakpoint = self.breakpoint(r);
        let broken = now > 0.0 && now <= breakpoint && was > breakpoint;
        let spent = now <= 0.0 && was > 0.0 && !self.fighting(r);
        if broken || spent {
            self.rout(r);
        }
    }

    /// Adds `amount` to the organization at `r`'s morale, up to its most.
    pub(crate) fn gain_morale(&mut self, r: usize, amount: f32) {
        let organization = &mut self.organizations[r];
        if amount > 0.0 && organization.morale_max > 0.0 {
            organization.morale = (organization.morale + amount).min(organization.morale_max);
        }
    }

    /// What the organization at `r` loses as the member at `u` dies, and
    /// what the organization of `by`, the unit that killed it if one did,
    /// wins for the kill.
    pub(crate) fn mourn(&mut self, u: usize, by: Option<Target>) {
        let rules = self.rules.morale_rules;
        let template = self.units[u].template.clone();
        let value = self
            .rules
            .morale_values
            .get(&template)
            .copied()
            .unwrap_or(0.0);
        let r = self.units[u].organization;
        self.lose_morale(r, value * rules.death_factor, false);
        let Some(Target::Unit(k)) = by else {
            return;
        };
        let killers = self.units[k].organization;
        if !self.hostile(killers, Piece::Organization(r)) {
            return;
        }
        let worth = |t: &Id| self.rules.combat_values.get(t).copied().unwrap_or(0.0);
        let (killer, victim) = (worth(&self.units[k].template), worth(&template));
        if killer <= 0.0 {
            return;
        }
        let [low, high] = rules.slayer_factor;
        let factor = ((victim / killer + 1.0) / 2.0).clamp(low, high);
        let gain = rules.slayer_base * factor;
        self.gain_morale(killers, gain);
    }

    /// Has the organization at `r` break and run: its owner is told, those
    /// engaging it take heart, and it drops everything to run.
    pub(crate) fn rout(&mut self, r: usize) {
        let owner = self.organizations[r].owner;
        self.notices.push(Notice::Routed {
            player: owner,
            organization: r,
        });
        self.hearten_breakers(r);
        let ticks = self.ticks;
        let until = ticks + (self.rules.morale_rules.routing_time / TICK).round() as u64;
        let organization = &mut self.organizations[r];
        organization.take_order(Order::Move);
        organization.engaged.clear();
        organization.bombarding = false;
        organization.mind = Mind::Routing {
            to: None,
            until,
            retry: ticks,
        };
        for u in organization.units.clone() {
            let unit = &mut self.units[u];
            unit.target = None;
            unit.closing = false;
        }
        self.run_away(r);
    }

    /// Gives each organization engaging the one at `r`, which is routing or
    /// running, the morale breaking it wins.
    pub(crate) fn hearten_breakers(&mut self, r: usize) {
        let rules = &self.rules.morale_rules;
        let fleeing = self.organizations[r].morale_max;
        let [low, high] = rules.breaker_factor;
        let base = rules.breaker_base;
        let gains: Vec<(usize, f32)> = (self.engagers(r))
            .filter(|&e| self.organizations[e].morale_max > 0.0)
            .map(|e| {
                let factor = (fleeing / self.organizations[e].morale_max).clamp(low, high);
                (e, base * factor)
            })
            .collect();
        for (e, gain) in gains {
            self.gain_morale(e, gain);
        }
    }

    /// Sends the routing organization at `r` to the best place it finds to
    /// run to, or has it wait to look again if it finds none.
    fn run_away(&mut self, r: usize) {
        let Mind::Routing { until, .. } = self.organizations[r].mind else {
            return;
        };
        let ticks = self.ticks;
        match self.rout_point(r) {
            Some(to) => {
                let here = self.centre(r);
                let facing = normalised([to[0] - here[0], to[1] - here[1]]);
                let facing = facing.unwrap_or(self.organizations[r].facing);
                self.organizations[r].mind = Mind::Routing {
                    to: Some(to),
                    until,
                    retry: ticks,
                };
                self.send(r, to, facing);
            }
            None => {
                let retry = ticks + (ROUT_RETRY / TICK).round() as u64;
                self.organizations[r].mind = Mind::Routing {
                    to: None,
                    until,
                    retry,
                };
            }
        }
    }

    /// The best place for the organization at `r` to run to, of those it
    /// can reach that it tries: away from those engaging it and toward its
    /// side's supply.
    fn rout_point(&mut self, r: usize) -> Option<[f32; 2]> {
        let rules = self.rules.morale_rules.rout;
        let here = self.centre(r);
        let owner = self.organizations[r].owner;
        let reach = self.organizations[r].control(&self.rules.guard_rules);
        let providers: Vec<usize> = (0..self.buildings.len())
            .filter(|&b| {
                let building = &self.buildings[b];
                building.supply.zone.is_some() && self.shares_supply(owner, building.owner)
            })
            .collect();
        let mut ways: Vec<f32> = Vec::new();
        for _ in 0..rules.random_candidates {
            ways.push(self.chance() * TAU);
        }
        let swing = rules.supply_zone_max_angle_delta.to_radians();
        for &b in &providers {
            let [x, y] = self.buildings[b].position;
            let toward = libm::atan2f(y - here[1], x - here[0]);
            for _ in 0..rules.supply_zone_candidates {
                ways.push(toward + (self.chance() * 2.0 - 1.0) * swing);
            }
        }
        let size = self.pathing.size();
        let mut points = Vec::new();
        for way in ways {
            let length = reach + self.chance() * rules.max_distance_delta;
            let (sin, cos) = libm::sincosf(way);
            let length = edge_cut(here, [cos, sin], length, size);
            // Kept a tile in, so the edge's own tiles count as on the map.
            let point = self
                .pathing
                .inside([here[0] + cos * length, here[1] + sin * length]);
            let fuzz = self.chance() * rules.fuzziness;
            points.push((point, fuzz));
        }
        let domain = self.leader_domain(r);
        let engagers: Vec<usize> = self.engagers(r).collect();
        let enemies: Vec<usize> = (0..self.organizations.len())
            .filter(|&e| !self.destroyed(e) && self.hostile(r, Piece::Organization(e)))
            .collect();
        let score = |w: &World, point: [f32; 2], fuzz: f32| {
            let mut score = rules.base_value;
            if !engagers.is_empty() {
                let each = engagers.iter().map(|&e| {
                    let there = w.centre(e);
                    let angle = angle_between(
                        [there[0] - here[0], there[1] - here[1]],
                        [point[0] - here[0], point[1] - here[1]],
                    );
                    let off = distance(there, point).min(rules.factor_enemy_dist_cap);
                    angle * rules.factor_enemy_angle + off * rules.factor_enemy_dist
                });
                score += each.sum::<f32>() / engagers.len() as f32;
            }
            let moved = distance(here, point).min(rules.factor_moved_dist_cap);
            score -= moved * rules.factor_moved_dist;
            let controlled = enemies.iter().any(|&e| {
                let reach = w.organizations[e].control(&w.rules.guard_rules);
                distance(w.centre(e), point) < reach
            });
            if controlled {
                score -= rules.penalty_enemy_zoc;
            }
            // `RoutPenaltyEnemyZOG` is left out: the spec doesn't say which
            // enemy zone it's for.
            if w.supplied(owner, point) {
                score += rules.bonus_friendly_zos;
            }
            let reaching = providers.iter().filter(|&&b| {
                (w.buildings[b].supply.zone.as_ref()).is_some_and(|z| z.holds(point))
            });
            score += rules.bonus_supply_in_range * reaching.count() as f32;
            let nearest = (providers.iter())
                .map(|&b| distance(w.buildings[b].position, point))
                .min_by(f32::total_cmp);
            if let Some(d) = nearest {
                score -= d.min(rules.factor_supply_dist_cap) * rules.factor_supply_dist;
            }
            score + fuzz
        };
        points
            .into_iter()
            .filter(|&(point, _)| path::reaches(&self.pathing, domain, here, point))
            .map(|(point, fuzz)| (point, score(self, point, fuzz)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(point, _)| point)
    }

    /// What the organization at `r`'s leader moves over, or its first
    /// surviving member's.
    fn leader_domain(&self, r: usize) -> tgmap::Domain {
        let organization = &self.organizations[r];
        let leader = organization.leader.map(|p| organization.units[p]);
        let u = leader
            .filter(|&u| self.units[u].alive())
            .or_else(|| self.survivors(r).next())
            .unwrap_or(organization.units[0]);
        self.units[u].domain
    }

    /// Plays a tick of morale: routs running their course, and on each
    /// organization's beat, what fighting takes and resting gives back.
    pub(crate) fn keep_morale(&mut self) {
        for r in 0..self.organizations.len() {
            if self.organizations[r].morale_max <= 0.0 || self.destroyed(r) {
                continue;
            }
            self.carry_on(r);
            if (self.ticks + r as u64).is_multiple_of(BEAT) {
                self.morale_beat(r);
            }
        }
    }

    /// Moves the organization at `r` on through a rout or a reform.
    fn carry_on(&mut self, r: usize) {
        let ticks = self.ticks;
        match self.organizations[r].mind {
            Mind::Routing { to, until, retry } => {
                let arrived = to.is_some() && !self.underway(r);
                if arrived || ticks >= until {
                    self.end_rout(r, arrived);
                } else if to.is_none() && ticks >= retry {
                    self.run_away(r);
                }
            }
            Mind::Reforming { until } => {
                let organization = &self.organizations[r];
                let leader = organization.leader.map(|p| organization.units[p]);
                let leaderless = leader.is_some_and(|u| !self.units[u].alive());
                let engaged = !organization.engaged.is_empty();
                if ticks >= until || !self.underway(r) || leaderless || engaged {
                    self.organizations[r].mind = Mind::Steady;
                }
            }
            Mind::Exhausted if self.organizations[r].morale > self.breakpoint(r) => {
                self.organizations[r].mind = Mind::Steady;
            }
            _ => {}
        }
    }

    /// Ends the organization at `r`'s rout: out of morale it's exhausted,
    /// otherwise it reforms where it ran to, or, out of time short of it,
    /// simply stops routing.
    fn end_rout(&mut self, r: usize, arrived: bool) {
        let ticks = self.ticks;
        let reform = (self.rules.morale_rules.reform_time / TICK).round() as u64;
        self.organizations[r].mind = match () {
            _ if self.organizations[r].morale <= 0.0 => {
                for u in self.survivors(r).collect::<Vec<_>>() {
                    self.units[u].halt();
                }
                Mind::Exhausted
            }
            _ if arrived => {
                self.take_places(r);
                Mind::Reforming {
                    until: ticks + reform,
                }
            }
            _ => Mind::Steady,
        };
    }

    /// A second of the organization at `r`'s morale: lost to being fought,
    /// regained resting, and broken again where it's still low.
    fn morale_beat(&mut self, r: usize) {
        let rules = self.rules.morale_rules;
        let engaged = self.engaged_by_enemy(r);
        if engaged && self.organizations[r].engaged_before {
            let organization = &self.organizations[r];
            let slots = organization.units.len().max(1) as f32;
            let empty = slots - self.survivors(r).count() as f32;
            let factor = 1.0 + (rules.engagement_loss_factor_max - 1.0) * empty / slots;
            let rate = 1.0 + self.morale_effects(r).map(|e| e.loss_rate).sum::<f32>();
            let loss = rules.engagement_loss * factor / rate.max(f32::EPSILON);
            self.lose_morale(r, loss, true);
        }
        self.organizations[r].engaged_before = engaged;
        if self.regaining(r) {
            let recovery = 1.0 + self.morale_effects(r).map(|e| e.recovery_rate).sum::<f32>();
            let gain = (rules.replenish + self.morale_bonus(r)) * recovery;
            self.gain_morale(r, gain);
        }
        let organization = &self.organizations[r];
        if organization.unpacked_into.is_some() || organization.routing() {
            return;
        }
        let morale = organization.morale;
        let steady = organization.mind == Mind::Steady;
        let idle = steady
            && organization.engaged.is_empty()
            && organization.order == Order::Move
            && !self.underway(r);
        let low = morale > 0.0 && morale <= self.breakpoint(r) && self.fighting(r);
        if low || (morale <= 0.0 && idle) {
            self.rout(r);
        }
    }

    /// Whether the organization at `r` is doing something that lets it
    /// regain morale: resting, guarding, reforming or exhausted.
    fn regaining(&self, r: usize) -> bool {
        let organization = &self.organizations[r];
        if organization.unpacked_into.is_some() {
            return false;
        }
        match organization.mind {
            Mind::Reforming { .. } | Mind::Exhausted => true,
            Mind::Routing { .. } => false,
            Mind::Steady => {
                let resting = organization.engaged.is_empty()
                    && organization.order == Order::Move
                    && !organization.bombarding
                    && !self.underway(r);
                resting || organization.guarding()
            }
        }
    }

    /// Whether the organization at `r` takes orders that move or fight it:
    /// none while it routs, is exhausted or is out of morale.
    pub(crate) fn may_steer(&self, r: usize) -> Result<(), Refused> {
        let Some(organization) = self.organizations.get(r) else {
            return Ok(());
        };
        let spent = organization.morale_max > 0.0 && organization.morale <= 0.0;
        match organization.mind {
            Mind::Routing { .. } => Err(Refused::Routing),
            Mind::Exhausted => Err(Refused::Exhausted),
            _ if spent => Err(Refused::Exhausted),
            _ => Ok(()),
        }
    }

    /// Ends a reform that an order to the organization at `r` cuts short.
    pub(crate) fn settle(&mut self, r: usize) {
        if let Some(organization) = self.organizations.get_mut(r)
            && matches!(organization.mind, Mind::Reforming { .. })
        {
            organization.mind = Mind::Steady;
        }
    }

    /// Whether `player` may order the organization at `r` to run: not while
    /// it's fighting with its morale too low.
    pub(crate) fn may_run(&self, player: u32, r: usize) -> Result<(), Refused> {
        self.may_command(player, r)?;
        let organization = &self.organizations[r];
        let penalty = self.rules.morale_rules.retreat_penalty;
        let low = organization.morale < self.breakpoint(r) + penalty + 1.0;
        match organization.morale_max > 0.0 && low && self.fighting(r) {
            true => Err(Refused::Unable),
            false => Ok(()),
        }
    }

    /// What running costs the organization at `r` and gives those fighting
    /// it, if it's fighting.
    pub(crate) fn flee(&mut self, r: usize) {
        if self.organizations[r].morale_max <= 0.0 || !self.fighting(r) {
            return;
        }
        self.hearten_breakers(r);
        let penalty = self.rules.morale_rules.retreat_penalty;
        self.lose_morale(r, penalty, true);
    }

    /// Whether `player` may order the organization at `r` to rout: only
    /// one with morale that's engaging enemies and can move.
    pub(crate) fn may_rout(&self, player: u32, r: usize) -> Result<(), Refused> {
        self.may_command(player, r)?;
        self.may_steer(r)?;
        let organization = &self.organizations[r];
        let moves = organization.speed > 0.0 && organization.unpacked_into.is_none();
        let engaging = !organization.engaged.is_empty();
        match organization.morale_max > 0.0 && moves && engaging {
            true => Ok(()),
            false => Err(Refused::Unable),
        }
    }
}

/// How far from `from` along the unit vector `way` a point `length` away
/// can be before it leaves a map `size` across.
fn edge_cut(from: [f32; 2], way: [f32; 2], length: f32, size: f32) -> f32 {
    let mut most = length;
    for i in 0..2 {
        let limit = match way[i] {
            w if w > 0.0 => (size - from[i]) / w,
            w if w < 0.0 => -from[i] / w,
            _ => continue,
        };
        most = most.min(limit.max(0.0));
    }
    most
}

/// The angle between two directions, in degrees from 0 to 180; 0 where
/// either is none.
fn angle_between(a: [f32; 2], b: [f32; 2]) -> f32 {
    match (normalised(a), normalised(b)) {
        (Some(a), Some(b)) => {
            let cos = (a[0] * b[0] + a[1] * b[1]).clamp(-1.0, 1.0);
            libm::acosf(cos).to_degrees()
        }
        _ => 0.0,
    }
}

impl crate::hash::Digest for Mind {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        match *self {
            Mind::Steady => 0u8.digest(h),
            Mind::Routing { to, until, retry } => (1u8, (to, (until, retry))).digest(h),
            Mind::Reforming { until } => (2u8, until).digest(h),
            Mind::Exhausted => 3u8.digest(h),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    use crate::{Command, Refused};

    /// Gives every unit in `w` a morale value of `value` and every
    /// organization a breakpoint of `breakpoint`, starting them at full.
    fn with_morale(mut w: World, value: f32, breakpoint: f32) -> World {
        for u in &w.units {
            w.rules.morale_values.insert(u.template.clone(), value);
        }
        for r in &w.organizations {
            w.rules
                .morale_breakpoints
                .insert(r.template.clone(), breakpoint);
        }
        for r in 0..w.organizations.len() {
            w.refigure_morale(r, true);
        }
        w
    }

    /// Player 1's and player 2's riflemen 10 apart, in each other's range
    /// and too weak to kill each other for a long while, worth 50 morale
    /// each with a breakpoint of 20.
    fn standoff() -> World {
        let w = battle(
            &[1, 2],
            vec![
                soldier(vec![rifle(12.0, 0.1)]),
                soldier(vec![rifle(12.0, 0.1)]),
            ],
            at_war(),
        );
        with_morale(w, 50.0, 20.0)
    }

    fn effect(f: impl FnOnce(&mut MoraleEffect)) -> MoraleEffect {
        let mut e = MoraleEffect::default();
        f(&mut e);
        e
    }

    /// Plays `w` until each side's rifleman engages the other.
    fn engaged(w: &mut World) {
        for _ in 0..50 {
            if w.engaged_by_enemy(0) && w.engaged_by_enemy(1) {
                return;
            }
            w.tick();
        }
        panic!("never engaged");
    }

    #[test]
    fn maximum_morale_counts_every_slot_and_the_bonuses() {
        let mut w = world(tgmap::Pathing::uniform(40, tgmap::Domain::Land));
        w.rules.morale_values.insert("soldier".into(), 5.0);
        w.rules.morale_breakpoints.insert("squad".into(), 4.0);
        w.rules.kinds.insert("squad".into(), kind(&["brave"]));
        let brave = effect(|e| {
            e.morale = 2.0;
            e.breakpoint = 1.0;
        });
        w.rules.morale_effects.insert("brave".into(), brave);
        w.refigure_morale(0, true);
        assert_eq!(w.organizations[0].morale_max, 12.0);
        assert_eq!(w.organizations[0].morale, 12.0);
        assert_eq!(w.breakpoint(0), 5.0);
        // A lost member's slot still counts, and the share is kept.
        w.organizations[0].morale = 6.0;
        w.units[1].died = Some(0);
        w.rules.morale_effects.clear();
        w.refigure_morale(0, false);
        assert_eq!(w.organizations[0].morale_max, 10.0);
        assert_eq!(w.organizations[0].morale, 5.0);
        // Without morale values it has no morale to lose.
        w.rules.morale_values.clear();
        w.refigure_morale(0, true);
        w.lose_morale(0, 1.0, false);
        assert_eq!(w.organizations[0].morale, 0.0);
        assert_eq!(w.organizations[0].mind, Mind::Steady);
    }

    #[test]
    fn being_engaged_wears_morale_down_each_second() {
        let mut w = standoff();
        engaged(&mut w);
        run(&mut w, 1.5);
        let before = w.organizations[0].morale;
        run(&mut w, 4.0);
        let lost = before - w.organizations[0].morale;
        assert!((lost - 2.0).abs() < 1e-3, "{lost}");
        // Half again on its loss rate loses a third less.
        let steady = effect(|e| e.loss_rate = 0.5);
        w.rules.morale_effects.insert("dug_in".into(), steady);
        w.give(Piece::Organization(0), "dug_in".into(), 100.0);
        let before = w.organizations[0].morale;
        run(&mut w, 3.0);
        let lost = before - w.organizations[0].morale;
        assert!((lost - 1.0).abs() < 1e-3, "{lost}");
    }

    #[test]
    fn empty_slots_make_a_fight_cost_more() {
        let mut w = standoff();
        // A second, lost, slot in the first organization.
        let mut lost = w.units[0].clone();
        lost.died = Some(0);
        w.units.push(lost);
        let u = w.units.len() - 1;
        w.organizations[0].units.push(u);
        w.refigure_morale(0, true);
        engaged(&mut w);
        run(&mut w, 1.5);
        let before = w.organizations[0].morale;
        run(&mut w, 2.0);
        // 0.5 × (1 + (3 − 1) × ½) a second.
        let lost = before - w.organizations[0].morale;
        assert!((lost - 2.0).abs() < 1e-3, "{lost}");
    }

    #[test]
    fn deaths_cost_their_organization_and_kills_win_by_combat_value() {
        let mut w = standoff();
        w.organizations[0].morale = 30.0;
        w.rules.combat_values.insert("soldier".into(), 4.0);
        w.mourn(1, Some(Target::Unit(0)));
        // A quarter of the victim's value lost, and for killing an equal
        // 0.5 × (1 + 1) ÷ 2 won.
        assert_eq!(w.organizations[1].morale, 50.0 - 12.5);
        assert_eq!(w.organizations[0].morale, 30.5);
        // A stronger killer wins less: 0.5 × (0.1 + 1) ÷ 2.
        w.rules.combat_values.insert("strong".into(), 40.0);
        w.units[0].template = "strong".into();
        w.mourn(1, Some(Target::Unit(0)));
        let morale = 30.5 + 0.275;
        assert!((w.organizations[0].morale - morale).abs() < 1e-4);
        // One worth nothing wins nothing.
        w.rules.combat_values.clear();
        w.mourn(1, Some(Target::Unit(0)));
        assert!((w.organizations[0].morale - morale).abs() < 1e-4);
        // A shot that kills does the same.
        let mut w = standoff();
        w.units[1].health = 0.05;
        run(&mut w, 1.0);
        assert!(!w.units[1].alive());
        assert!(w.organizations[1].morale <= 50.0 - 12.5);
    }

    #[test]
    fn weapons_with_morale_loss_take_it_from_each_unit_they_hit() {
        let mut w = standoff();
        w.units[1].fighter.as_mut().unwrap().weapons[0].morale_loss = 0.75;
        w.organizations[0].morale_max = 50.0;
        w.organizations[0].morale = 50.0;
        // A shot or two before the fight's own wear starts.
        for _ in 0..3 {
            w.tick();
        }
        let hit = w.units[0].health < 100.0;
        assert!(hit);
        assert_eq!(w.organizations[0].morale, 50.0 - 0.75);
        // Berserk shrugs it off, though not deaths.
        let berserk = effect(|e| e.berserk = true);
        w.rules.morale_effects.insert("berserk".into(), berserk);
        w.give(Piece::Organization(0), "berserk".into(), 100.0);
        w.lose_morale(0, 3.0, true);
        assert_eq!(w.organizations[0].morale, 50.0 - 0.75);
        w.mourn(0, None);
        assert_eq!(w.organizations[0].morale, 50.0 - 0.75 - 12.5);
    }

    #[test]
    fn resting_regains_morale_and_supply_adds_to_it() {
        let mut w = with_morale(supplied_division(), 5.0, 2.0);
        let r = 1;
        w.organizations[r].morale = 1.0;
        w.organizations[r].mind = Mind::Exhausted;
        let in_supply = w.organizations[r].in_supply;
        assert!(in_supply);
        run(&mut w, 1.0);
        let rate = w.rules.morale_rules.replenish + w.rules.supply_rules.morale_bonus;
        assert!((w.organizations[r].morale - (1.0 + rate)).abs() < 1e-3);
        // Back above its breakpoint, it takes orders again.
        w.tick();
        assert_eq!(w.organizations[r].mind, Mind::Steady);
        // Recovery rates multiply it.
        let rallied = effect(|e| e.recovery_rate = 1.0);
        w.rules.morale_effects.insert("rallied".into(), rallied);
        w.give(Piece::Organization(r), "rallied".into(), 100.0);
        let before = w.organizations[r].morale;
        run(&mut w, 1.0);
        let gained = w.organizations[r].morale - before;
        assert!((gained - 2.0 * rate).abs() < 1e-3, "{gained}");
    }

    #[test]
    fn morale_falling_to_the_breakpoint_routs_away_from_the_enemy() {
        let mut w = standoff();
        engaged(&mut w);
        w.organizations[0].morale = 20.2;
        w.organizations[1].morale = 40.0;
        w.take_notices();
        let start = distance(w.centre(0), w.centre(1));
        run(&mut w, 1.0);
        assert!(w.organizations[0].routing());
        assert!(w.organizations[0].engaged.is_empty());
        let routed = Notice::Routed {
            player: 1,
            organization: 0,
        };
        assert!(w.notices().contains(&routed));
        // Its enemy took heart: 3 × (50 ÷ 50).
        assert!(
            w.organizations[1].morale >= 43.0 - 0.5,
            "{}",
            w.organizations[1].morale
        );
        let to = [30.0, 30.0];
        let order = Command::Move {
            organization: 0,
            to,
        };
        assert_eq!(w.apply(1, order.clone()), Err(Refused::Routing));
        run(&mut w, 3.0);
        assert!(distance(w.centre(0), w.centre(1)) > start);
        // It doesn't take the enemy on as it goes.
        assert!(w.organizations[0].engaged.is_empty());
    }

    #[test]
    fn a_rout_that_gets_there_reforms_harmlessly_then_is_itself_again() {
        let mut w = standoff();
        w.rules.morale_rules.routing_time = 30.0;
        engaged(&mut w);
        w.organizations[0].morale = 20.2;
        run(&mut w, 1.0);
        assert!(w.organizations[0].routing());
        // Its enemy gone, nothing wears it down as it goes.
        w.destroy(1);
        let mut reformed = false;
        for _ in 0..300 {
            w.tick();
            if let Mind::Reforming { .. } = w.organizations[0].mind {
                reformed = true;
                break;
            }
        }
        assert!(reformed);
        assert!(w.organizations[0].harmless());
        // It takes orders, and regains morale.
        let before = w.organizations[0].morale;
        run(&mut w, 2.0);
        assert!(w.organizations[0].morale > before);
        let reform = w.rules.morale_rules.reform_time;
        run(&mut w, reform);
        assert_eq!(w.organizations[0].mind, Mind::Steady);
    }

    #[test]
    fn a_rout_out_of_time_simply_stops() {
        let mut w = standoff();
        w.rules.morale_rules.routing_time = 0.5;
        engaged(&mut w);
        w.organizations[0].morale = 20.2;
        run(&mut w, 1.6);
        let mind = w.organizations[0].mind;
        assert!(!w.organizations[0].routing(), "{mind:?}");
        assert!(!matches!(mind, Mind::Reforming { .. }), "{mind:?}");
    }

    #[test]
    fn spent_regiments_are_exhausted_after_their_rout_and_take_no_orders() {
        let mut w = standoff();
        w.organizations[0].morale = 1.0;
        // Out of a fight, it routs as it falls to 0.
        w.lose_morale(0, 5.0, false);
        assert!(w.organizations[0].routing());
        let routing = w.rules.morale_rules.routing_time;
        run(&mut w, routing + 1.0);
        assert_eq!(w.organizations[0].mind, Mind::Exhausted);
        assert!(!w.underway(0));
        let order = Command::Stop { organization: 0 };
        assert_eq!(w.apply(1, order), Err(Refused::Exhausted));
        // Disbanding is an order too; joining an HQ isn't held up by it.
        let order = Command::Disband { organization: 0 };
        assert_eq!(w.check(1, &order), Err(Refused::Exhausted));
    }

    #[test]
    fn falling_straight_to_nothing_mid_fight_waits_till_the_fight_ends() {
        let mut w = standoff();
        engaged(&mut w);
        w.lose_morale(0, 100.0, false);
        assert_eq!(w.organizations[0].morale, 0.0);
        assert!(!w.organizations[0].routing());
    }

    #[test]
    fn low_morale_withdraws_running_and_routing_is_an_order_in_a_fight() {
        let mut w = standoff();
        let rout = Command::Rout { organization: 0 };
        assert_eq!(w.check(1, &rout), Err(Refused::Unable));
        engaged(&mut w);
        w.organizations[0].morale = 20.5;
        let run_off = Command::Run {
            organization: 0,
            to: [10.0, 0.0],
        };
        assert_eq!(w.check(1, &run_off), Err(Refused::Unable));
        w.organizations[0].morale = 21.0;
        assert_eq!(w.check(1, &run_off), Ok(()));
        let enemy = w.organizations[1].morale;
        w.organizations[1].morale_max = 100.0;
        w.apply(1, rout).unwrap();
        assert!(w.organizations[0].routing());
        // Its enemy won 3 × (50 ÷ 100).
        assert!((w.organizations[1].morale - (enemy + 1.5)).abs() < 1e-3);
    }

    #[test]
    fn unpacked_organizations_lose_no_morale() {
        let mut w = standoff();
        w.organizations[0].unpacked_into = Some(0);
        w.lose_morale(0, 3.0, false);
        assert_eq!(w.organizations[0].morale, 50.0);
    }

    #[test]
    fn regiments_a_map_places_keep_the_share_of_morale_it_stored() {
        let mut w = standoff();
        let mut placed = raised_company(&mut w, 1);
        placed.morale = 0.5;
        w.rules.morale_values.insert("soldier".into(), 10.0);
        let r = w.add_organization(&placed);
        let organization = &w.organizations[r];
        assert!(organization.morale_max > 0.0);
        assert_eq!(organization.morale, organization.morale_max / 2.0);
    }

    #[test]
    fn out_of_morale_regiments_choose_formations_but_take_no_orders_that_move_them() {
        let mut w = standoff();
        w.organizations[0].mind = Mind::Exhausted;
        let formation = Command::Formation {
            organization: 0,
            formation: 0,
        };
        assert_eq!(w.check(1, &formation), Ok(()));
        // Even standing steady, with no morale it takes none.
        w.organizations[0].mind = Mind::Steady;
        w.organizations[0].morale = 0.0;
        let order = Command::Move {
            organization: 0,
            to: [20.0, 20.0],
        };
        assert_eq!(w.check(1, &order), Err(Refused::Exhausted));
    }

    #[test]
    fn routing_units_hold_their_fire() {
        let mut w = standoff();
        engaged(&mut w);
        w.organizations[0].morale = 20.2;
        run(&mut w, 1.0);
        assert!(w.organizations[0].routing());
        let health = w.units[1].health;
        run(&mut w, 3.0);
        assert_eq!(w.units[1].health, health);
        assert_eq!(w.units[0].target, None);
    }

    #[test]
    fn an_order_or_standing_still_ends_a_reform() {
        let mut w = standoff();
        w.organizations[0].mind = Mind::Reforming { until: 1000 };
        // Nowhere to go, it's done at once.
        w.tick();
        assert_eq!(w.organizations[0].mind, Mind::Steady);
        w.organizations[0].mind = Mind::Reforming { until: 1000 };
        let order = Command::Move {
            organization: 0,
            to: [20.0, 5.0],
        };
        w.apply(1, order).unwrap();
        assert_eq!(w.organizations[0].mind, Mind::Steady);
    }

    #[test]
    fn exhausted_regiments_fight_and_rout_if_their_morale_comes_back_low() {
        let mut w = standoff();
        engaged(&mut w);
        w.organizations[0].mind = Mind::Exhausted;
        w.organizations[0].morale = 10.0;
        run(&mut w, 1.0);
        assert!(w.organizations[0].routing());
    }

    #[test]
    fn only_a_regiment_engaging_enemies_may_be_ordered_to_rout() {
        let mut w = standoff();
        engaged(&mut w);
        // Shot at while it engages nothing itself.
        w.organizations[0].engaged.clear();
        assert!(w.fighting(0));
        let rout = Command::Rout { organization: 0 };
        assert_eq!(w.check(1, &rout), Err(Refused::Unable));
    }

    #[test]
    fn a_regiment_losing_its_last_man_doesnt_rout() {
        let mut w = standoff();
        w.organizations[0].morale = 20.5;
        w.take_notices();
        w.destroy(0);
        assert!(!w.organizations[0].routing());
        let routed = |n: &Notice| matches!(n, Notice::Routed { .. });
        assert!(!w.notices().iter().any(routed));
    }

    #[test]
    fn killing_a_friend_wins_no_morale() {
        let mut w = standoff();
        w.rules.combat_values.insert("soldier".into(), 4.0);
        w.organizations[0].morale = 30.0;
        w.organizations[1].owner = 1;
        w.mourn(1, Some(Target::Unit(0)));
        assert_eq!(w.organizations[0].morale, 30.0);
    }

    #[test]
    fn routing_units_go_on_reloading() {
        let mut w = standoff();
        w.organizations[0].mind = Mind::Routing {
            to: None,
            until: 1000,
            retry: 1000,
        };
        w.units[0].reloads = vec![1.0];
        w.tick();
        assert!(w.units[0].reloads[0] < 1.0);
    }

    #[test]
    fn regiments_at_the_map_edge_rout_along_it() {
        let mut w = standoff();
        let top = w.pathing.size() - 1.0;
        w.units[0].position = [20.0, top];
        w.units[1].position = [20.0, top - 10.0];
        engaged(&mut w);
        w.organizations[0].morale = 20.2;
        run(&mut w, 1.0);
        let Mind::Routing { to: Some(to), .. } = w.organizations[0].mind else {
            panic!("{:?}", w.organizations[0].mind);
        };
        let size = w.pathing.size();
        assert!(to.iter().all(|c| (0.0..size).contains(c)), "{to:?}");
    }

    #[test]
    fn rout_points_must_be_reachable() {
        let mut pathing = tgmap::Pathing::uniform(20, tgmap::Domain::Land);
        // A wall of water across the middle.
        for x in 0..20 {
            pathing.set(tgmap::Tile { x, y: 10 }, tgmap::Domain::Naval);
        }
        let below = [10.0, 5.0];
        let land = tgmap::Domain::Land;
        let size = pathing.size();
        let above = [10.0, size - 5.0];
        assert!(!path::reaches(&pathing, land, below, above));
        assert!(path::reaches(&pathing, land, below, [15.0, 8.0]));
    }

    #[test]
    fn rout_points_stay_on_the_map() {
        assert_eq!(edge_cut([10.0, 10.0], [-1.0, 0.0], 30.0, 100.0), 10.0);
        assert_eq!(edge_cut([10.0, 10.0], [0.0, 1.0], 30.0, 100.0), 30.0);
        assert!((angle_between([1.0, 0.0], [-1.0, 0.0]) - 180.0).abs() < 1e-3);
    }
}
