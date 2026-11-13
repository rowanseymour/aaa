//! Weapons: what they fire on, turrets, projectiles and the damage they
//! do.

use aaa_map::{Fighter, Id, Obstacle, Shape, TerrainEffect, Tile, Turret, Weapon};

use crate::{
    Footing, LANDINGS_KEPT, Landing, Order, Piece, Projectile, Relation, State, TICK, Target,
    TurretAim, World, bounding_radius, distance, supply, wrap,
};

impl TurretAim {
    /// Turns toward `yaw` by up to a tick's worth of `turret`'s turn rate,
    /// the short way round, returning whether it got there.
    fn turn(&mut self, turret: &Turret, yaw: f32) -> bool {
        let step = turret.turn_rate * TICK;
        let left = wrap(yaw - self.yaw);
        if left.abs() <= step {
            self.yaw = wrap(yaw);
            return true;
        }
        self.yaw = wrap(self.yaw + step.copysign(left));
        false
    }
}

/// Puts what's been hurt under siege until at least `until`.
fn besiege(besieged: &mut Option<u64>, until: u64) {
    *besieged = Some(besieged.map_or(until, |u| u.max(until)));
}

/// Damage a shot does to what it lands on, for the side that fired it.
struct Hit {
    target: Target,
    amount: f32,
    /// What fired it.
    from: Target,
    by: u32,
    /// For bombardment, its weapon's seconds between shots.
    bombard: Option<f32>,
}

/// The fraction of an area shot's damage that reaches `d` from where it
/// lands: full within [`Weapon::full_within`], falling
/// evenly to [`Weapon::edge_damage`] at the edge.
fn falloff(weapon: &Weapon, d: f32) -> f32 {
    let full = weapon.full_within.min(weapon.area);
    if d <= full || weapon.area <= full {
        return 1.0;
    }
    let along = (d - full) / (weapon.area - full);
    1.0 + (weapon.edge_damage - 1.0) * along
}

/// Health a shot of `weapon` takes from `target`, its damage scaled by
/// `scale`: defence comes off first, then resistance takes its share, and
/// every hit does at least 1. `ground` is what the terrain properties the
/// target holds add to both.
pub(crate) fn damage(
    weapon: &Weapon,
    target: &Fighter,
    ground: &[&TerrainEffect],
    scale: f32,
) -> f32 {
    let defense = target.defense + ground.iter().map(|e| e.defense).sum::<f32>();
    weapon
        .damage
        .iter()
        .map(|d| {
            let grounds = ground.iter().flat_map(|e| &e.resists);
            let resist = target.resists.get(&d.kind).copied().unwrap_or(0.0)
                + (grounds.filter(|(kind, _)| *kind == d.kind))
                    .map(|(_, amount)| amount)
                    .sum::<f32>();
            ((d.amount * scale - defense.max(0.0)) * (1.0 - resist)).max(1.0)
        })
        .sum()
}

impl World {
    /// Whatever can be fired on: alive units and standing buildings.
    pub(crate) fn targets(&self) -> impl Iterator<Item = Target> + '_ {
        let units = (0..self.units.len()).map(Target::Unit);
        let buildings = (0..self.buildings.len()).map(Target::Building);
        units
            .chain(buildings)
            .filter(|&t| self.fighter(t).is_some())
    }

    /// What a target can take and deal, if it's still there to fire on.
    pub(crate) fn fighter(&self, t: Target) -> Option<&Fighter> {
        match t {
            Target::Unit(u) => self.units[u]
                .fighter
                .as_ref()
                .filter(|_| self.units[u].alive()),
            Target::Building(b) => {
                let b = &self.buildings[b];
                b.standing().then_some(&b.fighter)
            }
        }
    }

    pub(crate) fn owner(&self, t: Target) -> u32 {
        match t {
            Target::Unit(u) => self.regiments[self.units[u].regiment].owner,
            Target::Building(b) => self.buildings[b].owner,
        }
    }

    pub(crate) fn position(&self, t: Target) -> [f32; 2] {
        match t {
            Target::Unit(u) => self.units[u].position,
            Target::Building(b) => self.buildings[b].position,
        }
    }

    /// How far a point is from a target: to a building's edge, a unit's
    /// middle.
    pub(crate) fn reach(&self, from: [f32; 2], t: Target) -> f32 {
        match t {
            Target::Unit(u) => distance(from, self.units[u].position),
            Target::Building(b) => self.buildings[b].distance(from),
        }
    }

    /// Fires every weapon that's ready at what it can hit, then takes the
    /// dead out of the fight, so that all hit each other at once. Only
    /// turrets fire on the move: anything else turns its user to aim.
    /// Returns, for each unit hit, its regiment and what fired.
    pub(crate) fn fight(&mut self) -> Vec<(usize, Target)> {
        let mut hits: Vec<Hit> = Vec::new();
        let mut launched = Vec::new();
        // Buildings that defend themselves sit idle until woken, and once
        // woken fire even while disabled.
        let defends = |b: usize| self.buildings[b].fighter.notice.is_some();
        let disabled = (0..self.buildings.len())
            .filter(|&b| defends(b) && self.buildings[b].state == State::Disabled)
            .map(Target::Building);
        let shooters: Vec<Target> = (self.targets().chain(disabled))
            .filter(|&t| match t {
                Target::Building(b) => !defends(b) || self.buildings[b].awake,
                Target::Unit(_) => true,
            })
            .collect();
        for s in shooters {
            // Nothing aiming reads turrets' yaw, so it's left out meanwhile.
            let mut turrets = match s {
                Target::Building(b) => std::mem::take(&mut self.buildings[b].turrets),
                Target::Unit(_) => Vec::new(),
            };
            let fighter = match s {
                Target::Unit(u) => self.units[u].fighter.as_ref(),
                Target::Building(b) => Some(&self.buildings[b].fighter),
            };
            let fighter = fighter.expect("shooters can fight");
            let (mut reloads, mut target, mut shot, moving) = match s {
                Target::Unit(u) => {
                    let u = &self.units[u];
                    (u.reloads.clone(), u.target, u.shot, u.moving())
                }
                Target::Building(b) => {
                    let b = &self.buildings[b];
                    (b.reloads.clone(), b.target, b.shot, false)
                }
            };
            let by = self.owner(s);
            for aim in &mut turrets {
                aim.idle += TICK;
            }
            for (w, weapon) in fighter.weapons.iter().enumerate() {
                reloads[w] = (reloads[w] - TICK).max(0.0);
                if reloads[w] > 0.0 || moving && !weapon.turret {
                    continue;
                }
                let Some(t) = self.aim(s, weapon, target) else {
                    continue;
                };
                target = Some(t);
                // A building's turret must point at its target first.
                if let (Target::Building(b), Some(i)) = (s, weapon.aimed_by) {
                    let building = &self.buildings[b];
                    let turret = &building.fighter.turrets[i];
                    turrets[i].idle = 0.0;
                    let pointing = match building.yaw_to(turret, self.position(t)) {
                        Some(yaw) => turrets[i].turn(turret, yaw),
                        None => true,
                    };
                    if !pointing {
                        continue;
                    }
                }
                shot = Some((self.ticks, w));
                reloads[w] = weapon.recharge;
                match weapon.flight {
                    Some(flight) => launched.push(self.launch(s, by, w, weapon, flight, t)),
                    None => {
                        let bombard = self.bombards(s, weapon);
                        self.strike(weapon, self.position(t), t, (s, by), bombard, &mut hits)
                    }
                }
            }
            match s {
                Target::Unit(u) => {
                    let u = &mut self.units[u];
                    (u.reloads, u.target, u.shot) = (reloads, target, shot);
                }
                Target::Building(b) => {
                    let b = &mut self.buildings[b];
                    for (aim, turret) in turrets.iter_mut().zip(&b.fighter.turrets) {
                        if turret.reset.is_some_and(|r| aim.idle > r) {
                            aim.turn(turret, turret.start);
                        }
                    }
                    (b.reloads, b.target, b.shot, b.turrets) = (reloads, target, shot, turrets);
                }
            }
        }
        self.flying.extend(launched);
        let ticks = self.ticks;
        let (landed, flying): (Vec<_>, Vec<_>) = std::mem::take(&mut self.flying)
            .into_iter()
            .partition(|p| p.lands <= ticks);
        self.flying = flying;
        self.landings.retain(|l| l.tick + LANDINGS_KEPT > ticks);
        for p in landed {
            self.strike(
                &p.shot,
                p.at,
                p.target,
                (p.from, p.by),
                p.bombard,
                &mut hits,
            );
            self.landings.push(Landing {
                tick: ticks,
                from: p.from,
                weapon: p.weapon,
                target: p.target,
                at: p.at,
            });
        }
        let mut fired_on = Vec::new();
        for hit in hits {
            if let Target::Unit(u) = hit.target {
                let r = self.units[u].regiment;
                match hit.bombard {
                    // Bombardment keeps what it hurts out of supply rather
                    // than drawing its fire.
                    Some(recharge) if self.units[u].alive() => self.shell(r, hit.from, recharge),
                    Some(_) => {}
                    None => fired_on.push((r, hit.from)),
                }
            }
            self.land(hit);
        }
        let gone = |w: &World, t: Option<Target>| t.is_some_and(|t| w.fighter(t).is_none());
        for u in 0..self.units.len() {
            if gone(self, self.units[u].target) {
                self.units[u].target = None;
            }
        }
        for b in 0..self.buildings.len() {
            if gone(self, self.buildings[b].target) {
                self.buildings[b].target = None;
            }
        }
        fired_on
    }

    /// The projectile `s`, of player `by`, fires with its weapon `w` at
    /// `t`: flying for its life if it has one, else from `s` to where it
    /// aims at its speed, and aimed ahead of a moving target by as far as
    /// it moves in that time. Heights aren't simulated, so distances are
    /// across the ground.
    fn launch(
        &self,
        s: Target,
        by: u32,
        w: usize,
        weapon: &Weapon,
        flight: aaa_map::Flight,
        t: Target,
    ) -> Projectile {
        let aim = self.position(t);
        let seconds = match (flight.life > 0.0, flight.speed > 0.0) {
            (true, _) => flight.life,
            (false, true) => distance(self.position(s), aim) / flight.speed,
            (false, false) => 0.0,
        };
        let velocity = match t {
            Target::Unit(u) => {
                let u = &self.units[u];
                [0, 1].map(|i| (u.position[i] - u.previous[i]) / TICK)
            }
            Target::Building(_) => [0.0; 2],
        };
        Projectile {
            from: s,
            by,
            weapon: w,
            shot: weapon.clone(),
            target: t,
            at: [0, 1].map(|i| aim[i] + velocity[i] * seconds),
            lands: self.ticks + (seconds / TICK - 1e-4).ceil().max(0.0) as u64,
            bombard: self.bombards(s, weapon),
        }
    }

    /// Takes a hit's damage off what it lands on. A building whose health
    /// runs out is captured by the hit's owner if it can be, else destroyed
    /// if it can be, else disabled.
    fn land(&mut self, hit: Hit) {
        let ticks = self.ticks;
        let window = (self.rules.siege_seconds / TICK).round() as u64;
        match hit.target {
            Target::Unit(u) => {
                let owner = self.regiments[self.units[u].regiment].owner;
                let enemy = self.sides.relation(hit.by, owner) == Relation::Enemy;
                let unit = &mut self.units[u];
                if !unit.alive() {
                    return;
                }
                if enemy && hit.amount > 0.0 {
                    besiege(&mut unit.besieged, ticks + window);
                }
                unit.health -= hit.amount;
                if unit.health <= 0.0 {
                    unit.die(ticks);
                    self.lost_member(u);
                }
            }
            Target::Building(b) => {
                let building = &mut self.buildings[b];
                // Nothing a side fires hurts its own, such as what it took
                // over earlier in the tick.
                if !building.standing() || building.owner == hit.by {
                    return;
                }
                if self.sides.relation(hit.by, building.owner) == Relation::Enemy
                    && hit.amount > 0.0
                {
                    // A bombarder holds it for as long as it keeps a
                    // regiment out of supply, if that's longer.
                    let held = hit.bombard.map_or(0.0, |recharge| {
                        self.rules.supply_rules.bombard_timeout + recharge
                    });
                    let held = (held / TICK).round() as u64;
                    besiege(&mut building.besieged, ticks + window.max(held));
                }
                building.health -= hit.amount;
                if building.health <= 0.0 {
                    self.fall(b, Some(hit.by));
                }
            }
        }
    }

    /// Takes from each building what its own properties cost it, a second
    /// at a time from when it was put on the map: plain loss, whatever its
    /// defence.
    pub(crate) fn decay(&mut self) {
        for b in 0..self.buildings.len() {
            let building = &self.buildings[b];
            let age = self.ticks - building.created;
            let rate = self.rules.decays.get(&building.template);
            let Some(&rate) = rate.filter(|_| building.standing()) else {
                continue;
            };
            if age > 0 && age.is_multiple_of(supply::BEAT) {
                self.buildings[b].health -= rate;
                if self.buildings[b].health <= 0.0 {
                    self.fall(b, None);
                }
            }
        }
    }

    /// Ends sieges no enemy has kept up. Each thing looks once a second,
    /// on its own second, so a siege outlasts its last hit by up to one.
    pub(crate) fn lift_sieges(&mut self) {
        let ticks = self.ticks;
        let lifts = |besieged: &mut Option<u64>, offset: u64| {
            let due = (ticks + offset).is_multiple_of(supply::BEAT);
            if due && besieged.is_some_and(|until| until <= ticks) {
                *besieged = None;
            }
        };
        for building in &mut self.buildings {
            lifts(&mut building.besieged, building.created);
        }
        for unit in &mut self.units {
            lifts(&mut unit.besieged, unit.id as u64);
        }
    }

    /// What `s` fires `weapon` at: the best it can, armed enemies first,
    /// then what can be captured or destroyed, then the rest, and the
    /// nearest of them; or the one it fired on last as long as it's as good.
    /// A unit whose regiment is pursuing an enemy fires on that enemy
    /// first whenever it can.
    fn aim(&self, s: Target, weapon: &Weapon, last: Option<Target>) -> Option<Target> {
        if let Target::Building(b) = s
            && self.buildings[b].fighter.notice.is_some()
        {
            return self.defend(b, weapon, last);
        }
        let from = self.position(s);
        let quarry = match s {
            Target::Unit(u) => match self.regiments[self.units[u].regiment].order {
                Order::Pursue(e) => Some(e),
                _ => None,
            },
            Target::Building(_) => None,
        };
        let of_quarry = |t: Target| match (quarry, t) {
            (Some(e), Target::Unit(v)) => self.units[v].regiment == e,
            _ => false,
        };
        let pursued = self
            .targets()
            .filter(|&t| of_quarry(t) && self.can_hit(s, weapon, t));
        let nearest =
            |a: &Target, b: &Target| self.reach(from, *a).total_cmp(&self.reach(from, *b));
        if let Some(t) = last.filter(|&t| of_quarry(t) && self.can_hit(s, weapon, t)) {
            return Some(t);
        }
        if let Some(t) = pursued.min_by(nearest) {
            return Some(t);
        }
        let best = self
            .targets()
            .filter(|&t| self.can_hit(s, weapon, t))
            .max_by(|&a, &b| {
                self.rank(a)
                    .cmp(&self.rank(b))
                    .then(self.reach(from, b).total_cmp(&self.reach(from, a)))
            })?;
        match last.filter(|&t| self.can_hit(s, weapon, t)) {
            Some(t) if self.rank(t) >= self.rank(best) => Some(t),
            _ => Some(best),
        }
    }

    /// Has each building that defends itself notice, for good, what comes
    /// into its zone that one of its weapons could aim at then: a unit's
    /// whole regiment. Stepping onto another of the zone's tiles, or
    /// changing sides there, is coming in again. Noticing anything once the
    /// map has loaded wakes it; the first look stands for the map loading.
    pub(crate) fn notice(&mut self) {
        for b in 0..self.buildings.len() {
            let building = &self.buildings[b];
            let Some(near) = building.fighter.notice else {
                continue;
            };
            // What's there when it stands again comes in afresh.
            if building.gone() {
                self.buildings[b].inside.clear();
                continue;
            }
            let s = Target::Building(b);
            let zone = self.pathing.zone(building.position, near);
            let inside: Vec<(Target, Footing)> = self
                .targets()
                .filter(|&t| self.in_zone(&zone, t))
                .map(|t| (t, (Tile::at(self.position(t)), self.owner(t))))
                .collect();
            let seen: Vec<Piece> = (inside.iter())
                .filter(|(t, at)| building.inside.get(t) != Some(at))
                .filter(|&&(t, _)| {
                    let weapons = &building.fighter.weapons;
                    weapons
                        .iter()
                        .any(|w| self.can_target(s, w, t) && self.in_arc(b, w, t))
                })
                .map(|&(t, _)| self.piece(t))
                .collect();
            let building = &mut self.buildings[b];
            building.inside = inside.into_iter().collect();
            building.awake |= self.loaded && !seen.is_empty();
            for e in seen {
                if !building.noticed.contains(&e) {
                    building.noticed.push(e);
                }
            }
        }
        self.loaded = true;
    }

    /// The regiment or building a target is, or is in.
    pub(crate) fn piece(&self, t: Target) -> Piece {
        match t {
            Target::Unit(u) => Piece::Regiment(self.units[u].regiment),
            Target::Building(b) => Piece::Building(b),
        }
    }

    /// What a building that defends itself fires a weapon on: of what it's
    /// noticed and the weapon can hit, its last target while that's worth
    /// keeping, else the best ranked, nearest first.
    fn defend(&self, b: usize, weapon: &Weapon, last: Option<Target>) -> Option<Target> {
        let s = Target::Building(b);
        let from = self.buildings[b].position;
        let noticed = &self.buildings[b].noticed;
        let valid = |t: Target| {
            noticed.contains(&self.piece(t))
                && self.can_hit(s, weapon, t)
                && self.in_arc(b, weapon, t)
                && self.defended_rank(t) > 0
        };
        if let Some(t) = last.filter(|&t| valid(t) && self.defended_rank(t) >= 2) {
            return Some(t);
        }
        let nearer = |a: Target, b: Target| {
            distance(from, self.position(b)).total_cmp(&distance(from, self.position(a)))
        };
        self.targets()
            .filter(|&t| valid(t))
            .max_by(|&a, &b| (self.defended_rank(a).cmp(&self.defended_rank(b))).then(nearer(a, b)))
    }

    /// Whether building `b`'s turret for `weapon`, if it has one, can be
    /// aimed at `t`'s centre.
    fn in_arc(&self, b: usize, weapon: &Weapon, t: Target) -> bool {
        let building = &self.buildings[b];
        weapon
            .aimed_by
            .is_none_or(|i| building.in_arc(&building.fighter.turrets[i], self.position(t)))
    }

    /// How much a target is worth a defending building's fire before
    /// others; 0 for never.
    fn defended_rank(&self, t: Target) -> u8 {
        let Some(f) = self.fighter(t) else {
            return 0;
        };
        let armed = !f.weapons.is_empty();
        if let Target::Unit(u) = t {
            let unit = &self.units[u];
            let regiment = &self.regiments[unit.regiment];
            // A unit in no regiment is one of its own template.
            let lone = unit.template == regiment.template;
            if regiment.aggressive() || armed && lone {
                return 3;
            }
        }
        match (armed, f.capturable || f.destructible) {
            (true, _) => 2,
            (false, true) => 1,
            (false, false) => 0,
        }
    }

    /// How much a target is worth firing on before others.
    fn rank(&self, t: Target) -> u8 {
        let Some(f) = self.fighter(t) else {
            return 0;
        };
        match (f.weapons.is_empty(), f.capturable || f.destructible) {
            (false, _) => 3,
            (true, true) => 2,
            (true, false) => 1,
        }
    }

    pub(crate) fn can_hit(&self, s: Target, weapon: &Weapon, t: Target) -> bool {
        // What stands on the other is in range, and never too near.
        let near_enough = self.gap(s, t).is_none_or(|gap| {
            gap <= weapon.range && (weapon.min_range <= 0.0 || gap > weapon.min_range)
        });
        near_enough && self.can_target(s, weapon, t)
    }

    /// The outline weapons' ranges run to and from, where `t` stands; a
    /// point for what can't fight.
    fn outline(&self, t: Target) -> Obstacle {
        let (shape, facing) = match t {
            Target::Unit(u) => {
                let unit = &self.units[u];
                (unit.fighter.as_ref().map(|f| f.target), unit.facing)
            }
            Target::Building(b) => {
                let b = &self.buildings[b];
                (Some(b.fighter.target), b.facing)
            }
        };
        let shape = shape.unwrap_or(Shape::Point);
        Obstacle {
            position: self.position(t),
            facing,
            shape,
            soft: shape,
        }
    }

    /// How far apart `s` and `t` are, edge to edge, for `s` to fire on
    /// `t`; `None` where one stands on the other. Between a rectangle and
    /// anything else, the other counts as the circle round it: of two, a
    /// square beside a rectangle does, else the smaller, or `t` if they're
    /// as big.
    fn gap(&self, s: Target, t: Target) -> Option<f32> {
        let (from, to) = (self.outline(s), self.outline(t));
        // A rectangle before a square before the rest, then the bigger.
        let rank = |of: Target, o: &Obstacle| {
            let square = self.fighter(of).is_some_and(|f| f.target_square);
            match o.shape {
                Shape::Rect { .. } if square => 1,
                Shape::Rect { .. } => 2,
                _ => 0,
            }
        };
        let (kept, other) = match (rank(s, &from), rank(t, &to)) {
            (0, 0) => {
                let apart = distance(from.position, to.position);
                return Some(apart - bounding_radius(from.shape) - bounding_radius(to.shape));
            }
            (a, b) if a < b => (to, from),
            (a, b) if a == b && bounding_radius(to.shape) > bounding_radius(from.shape) => {
                (to, from)
            }
            _ => (from, to),
        };
        let apart = kept.distance(other.position);
        (apart > 0.0).then(|| apart - bounding_radius(other.shape))
    }

    /// Whether `weapon` fires on what `t` is, wherever it stands.
    pub(crate) fn can_target(&self, s: Target, weapon: &Weapon, t: Target) -> bool {
        t != s && self.fighter(t).is_some() && self.matches(s, &weapon.targets, t)
    }

    /// Whether what `t` is, a fighter with its flags and its side's
    /// standing toward `s`'s, has the flags `targets` asks for, alive or
    /// not.
    pub fn matches(&self, s: Target, targets: &aaa_map::Targets, t: Target) -> bool {
        self.fits(self.owner(s), targets, t)
    }

    /// As [`World::matches`], for something of player `by`'s, which a shot
    /// in flight stays whatever becomes of what fired it.
    fn fits(&self, by: u32, targets: &aaa_map::Targets, t: Target) -> bool {
        let fighter = match t {
            Target::Unit(u) => self.units[u].fighter.as_ref(),
            Target::Building(b) => Some(&self.buildings[b].fighter),
        };
        let Some(fighter) = fighter else {
            return false;
        };
        let side = self.standing_flag(by, self.owner(t));
        targets.allow(fighter.flags.iter().chain(std::iter::once(&side)))
    }

    /// The flag something of `owner`'s has for [`aaa_map::Targets`] of
    /// `by`'s to match, by how the sides stand.
    pub(crate) fn standing_flag(&self, by: u32, owner: u32) -> Id {
        Id::from(match self.sides.relation(by, owner) {
            Relation::Enemy => "enemy",
            Relation::Ally | Relation::Itself => "friend",
            _ => "neutral",
        })
    }

    /// Records the damage a shot fired by `from` of `by`'s side, as
    /// bombardment or not, landing at `at` and aimed at `t`, does to what
    /// it hits: `t`, or for an area
    /// shot whatever around `at` the weapon fires on, but never `from`.
    fn strike(
        &self,
        weapon: &Weapon,
        at: [f32; 2],
        t: Target,
        (from, by): (Target, u32),
        bombard: bool,
        hits: &mut Vec<Hit>,
    ) {
        let bombard = bombard.then_some(weapon.recharge);
        if weapon.area <= 0.0 {
            if let Some(f) = self.fighter(t) {
                hits.push(Hit {
                    target: t,
                    amount: damage(weapon, f, &self.terrain_effects(t), 1.0),
                    from,
                    by,
                    bombard,
                });
            }
            return;
        }
        let mut reached = Vec::new();
        for target in self.targets() {
            let f = self.fighter(target).expect("targets can fight");
            // To the circle round its soft shape, however it's turned.
            let middle = distance(at, self.position(target));
            let d = (middle - bounding_radius(f.soft)).max(0.0);
            if d > weapon.area || target == from || !self.fits(by, &weapon.targets, target) {
                continue;
            }
            let ground = self.terrain_effects(target);
            reached.push((
                middle,
                target,
                damage(weapon, f, &ground, falloff(weapon, d)),
            ));
        }
        // Those nearest by their middles, where there's a most it hits.
        if weapon.max_targets > 0 {
            reached.sort_by(|a, b| a.0.total_cmp(&b.0));
            reached.truncate(weapon.max_targets as usize);
        }
        hits.extend(reached.into_iter().map(|(_, target, amount)| Hit {
            target,
            amount,
            from,
            by,
            bombard,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    use std::f32::consts::{PI, TAU};

    use crate::{Command, NotYours, Piece, Target, World};

    /// A player 2 pillbox at (10, 24), noticing within 20, whose rifle
    /// can't hit aircraft, and unarmed soldiers of two of player 1's
    /// regiments: the first in its zone, the second, of `flags`, far off.
    fn sentry(flags: &[&str]) -> World {
        let mut far = soldier(Vec::new());
        far.flags = flags.iter().map(|&f| Id::from(f)).collect();
        let mut w = battle(&[1, 1], vec![soldier(Vec::new()), far], at_war());
        w.units[1].position = [40.0, 40.0];
        let mut pillbox = building(20, 2, [10.0, 24.0]);
        let mut rifle = rifle(12.0, 10.0);
        rifle.targets.excluded.push(Id::from("air"));
        pillbox.fighter.weapons = vec![rifle];
        pillbox.fighter.notice = Some(20.0);
        with_buildings(w, &[pillbox])
    }

    /// A pillbox south of the siege's soldier, facing it, whose rifle is on
    /// a turret that starts `start` degrees round, turns 90° a second and
    /// aims within `sweep` degrees.
    fn turreted(start: f32, sweep: f32) -> World {
        let mut pillbox = building(20, 2, [10.0, 24.0]);
        pillbox.fighter.weapons = vec![Weapon {
            aimed_by: Some(0),
            ..rifle(12.0, 10.0)
        }];
        pillbox.fighter.notice = Some(20.0);
        pillbox.fighter.turrets = vec![Turret {
            turn_rate: 90f32.to_radians(),
            sweep: sweep.to_radians(),
            start: start.to_radians(),
            reset: None,
            offset: [0.0; 2],
        }];
        let mut w = siege(&[pillbox]);
        w.units[0].fighter = Some(soldier(Vec::new()));
        // Already woken, as if saved so, since the soldier starts in reach.
        w.buildings[0].awake = true;
        w
    }

    #[test]
    fn enemies_in_range_fire_on_each_other_until_one_dies() {
        let mut w = battle(
            &[1, 2],
            vec![
                soldier(vec![rifle(12.0, 30.0)]),
                soldier(vec![rifle(12.0, 20.0)]),
            ],
            at_war(),
        );
        w.tick();
        assert_eq!(w.units[0].target, Some(Target::Unit(1)));
        assert_eq!(w.units[0].health, 80.0);
        assert_eq!(w.units[1].health, 70.0);
        // Every 2s: the second is down to 10 after three volleys and dies
        // to the fourth, while the first has taken 80.
        run(&mut w, 6.0);
        assert!(w.units[0].alive() && !w.units[1].alive());
        assert_eq!(w.units[0].health, 20.0);
        assert!(w.destroyed(1) && !w.destroyed(0));
        assert_eq!(w.units[0].target, None);
        assert_eq!(w.order_move(2, 1, [0.0, 0.0]), Err(NotYours));
    }

    #[test]
    fn projectiles_hit_when_their_flight_ends() {
        // 10 apart at 25 a second takes 0.4s; a life overrides the speed.
        let gun = |life| Weapon {
            flight: Some(aaa_map::Flight {
                kind: aaa_map::FlightKind::Straight,
                speed: 25.0,
                life,
            }),
            ..rifle(12.0, 30.0)
        };
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![gun(0.0)]), soldier(vec![gun(1.0)])],
            at_war(),
        );
        w.tick();
        assert_eq!(w.units[0].shot, Some((0, 0)));
        assert_eq!((w.units[0].health, w.units[1].health), (100.0, 100.0));
        assert_eq!(w.flying.len(), 2);
        run(&mut w, 0.3);
        assert_eq!((w.units[0].health, w.units[1].health), (100.0, 100.0));
        w.tick();
        assert_eq!((w.units[0].health, w.units[1].health), (100.0, 70.0));
        let landed = &w.landings[0];
        assert_eq!(
            (landed.tick, landed.from, landed.target),
            (4, Target::Unit(0), Target::Unit(1))
        );
        run(&mut w, 0.6);
        assert_eq!(w.units[0].health, 70.0);
        assert_eq!(w.landings.len(), 2);
    }

    #[test]
    fn only_turrets_fire_on_the_move() {
        let turret = Weapon {
            turret: true,
            ..rifle(12.0, 10.0)
        };
        for (weapon, fires_on_the_move) in [(rifle(12.0, 10.0), false), (turret, true)] {
            let mut w = battle(
                &[1, 2],
                vec![soldier(vec![weapon]), soldier(Vec::new())],
                at_war(),
            );
            // 2 units a second toward the enemy, in range all the way.
            w.apply(
                1,
                Command::Move {
                    regiment: 0,
                    to: [10.0, 14.0],
                },
            )
            .unwrap();
            run(&mut w, 1.5);
            assert!(w.units[0].moving());
            assert_eq!(w.units[1].health < 100.0, fires_on_the_move);
            run(&mut w, 1.0);
            assert!(!w.units[0].moving());
            assert!(w.units[1].health < 100.0);
        }
    }

    #[test]
    fn weapons_hold_fire_on_friends_what_they_exclude_and_what_is_out_of_range() {
        let mut tank = soldier(vec![rifle(12.0, 30.0)]);
        tank.flags = vec![Id::from("tank")];
        // 1 and 3 are allies, 2 a tank the rifles can't hurt, and 4 out of
        // the rifles' range.
        let mut w = battle(
            &[1, 3, 2, 2],
            vec![
                soldier(vec![rifle(12.0, 30.0)]),
                soldier(vec![rifle(12.0, 30.0)]),
                tank,
                soldier(Vec::new()),
            ],
            at_war(),
        );
        run(&mut w, 10.0);
        assert_eq!(w.units[2].health, 100.0);
        assert_eq!(w.units[3].health, 100.0);
        // The tank's own rifle has no such limit.
        assert!(!w.units[1].alive());
    }

    #[test]
    fn defence_then_resistance_soften_hits() {
        let mut tank = soldier(Vec::new());
        tank.defense = 6.0;
        tank.resists = [(Id::from("gunfire"), 0.6)].into();
        // (12 - 6) * 0.4
        assert!((damage(&rifle(12.0, 12.0), &tank, &[], 1.0) - 2.4).abs() < 1e-5);
        assert_eq!(damage(&rifle(12.0, 5.0), &tank, &[], 1.0), 1.0);
        assert!((damage(&rifle(12.0, 50.0), &tank, &[], 0.5) - 7.6).abs() < 1e-5);
    }

    #[test]
    fn area_shots_fall_off_toward_their_edge() {
        let shell = Weapon {
            area: 6.0,
            full_within: 2.0,
            edge_damage: 0.5,
            ..rifle(30.0, 40.0)
        };
        assert_eq!(falloff(&shell, 1.0), 1.0);
        assert_eq!(falloff(&shell, 4.0), 0.75);
        assert_eq!(falloff(&shell, 6.0), 0.5);
    }

    #[test]
    fn area_shots_hit_only_what_their_weapon_fires_on() {
        let shell = || Weapon {
            area: 6.0,
            full_within: 2.0,
            edge_damage: 0.5,
            ..rifle(30.0, 40.0)
        };
        // A friend 2 from where the shell lands, on an enemy.
        let mut w = battle(
            &[1, 1, 2],
            vec![
                soldier(vec![shell()]),
                soldier(Vec::new()),
                soldier(Vec::new()),
            ],
            at_war(),
        );
        w.units[1].position = [10.0, 28.0];
        w.tick();
        assert_eq!(w.units[1].health, 100.0);
        assert_eq!(w.units[2].health, 60.0);
        // An enemy near enough for the shell to reach back to what fired it.
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![shell()]), soldier(Vec::new())],
            at_war(),
        );
        w.units[1].position = [10.0, 12.0];
        w.tick();
        assert_eq!(w.units[0].health, 100.0);
        assert_eq!(w.units[1].health, 60.0);
    }

    #[test]
    fn buildings_fire_from_their_edge() {
        // The soldier's 14 from its middle, out of the rifle's 12, but 11
        // from its edge.
        let mut pillbox = building(20, 2, [10.0, 24.0]);
        pillbox.fighter.weapons = vec![rifle(12.0, 10.0)];
        let mut w = siege(&[pillbox]);
        w.units[0].fighter = Some(soldier(Vec::new()));
        w.tick();
        assert_eq!(w.buildings[0].target, Some(Target::Unit(0)));
        assert_eq!(w.units[0].health, 90.0);
    }

    #[test]
    fn defenders_fire_only_on_what_they_noticed_but_wherever_it_goes() {
        // The soldier's 14 from its middle, so out of the 10 it notices
        // within, though in its rifle's 12 from its edge.
        let mut pillbox = building(20, 2, [10.0, 24.0]);
        pillbox.fighter.weapons = vec![rifle(12.0, 10.0)];
        pillbox.fighter.notice = Some(10.0);
        let mut w = siege(&[pillbox]);
        w.units[0].fighter = Some(soldier(Vec::new()));
        run(&mut w, 3.0);
        assert_eq!(w.units[0].health, 100.0);
        w.units[0].position = [10.0, 16.0];
        w.tick();
        assert_eq!(w.buildings[0].noticed, [Piece::Regiment(0)]);
        assert_eq!(w.units[0].health, 90.0);
        // Spotting and firing on an enemy isn't being under siege; being
        // hit by one is.
        assert!(w.buildings[0].besieged.is_none());
        assert!(w.units[0].besieged.is_some());
        w.units[0].position = [10.0, 10.0];
        run(&mut w, 2.0);
        assert_eq!(w.units[0].health, 80.0);
    }

    #[test]
    fn defenders_leave_alone_what_the_map_placed_near_them() {
        let mut w = sentry(&["infantry"]);
        run(&mut w, 5.0);
        assert_eq!(w.buildings[0].noticed, [Piece::Regiment(0)]);
        assert!(!w.buildings[0].awake);
        assert!(w.buildings[0].target.is_none());
        assert_eq!(w.units[0].health, 100.0);
    }

    #[test]
    fn defenders_woken_fire_on_what_they_noticed_as_the_map_loaded() {
        let mut w = sentry(&["infantry"]);
        run(&mut w, 2.0);
        assert!(!w.buildings[0].awake);
        // The second soldier walking in wakes it, and it fires on the
        // first, nearer.
        w.units[1].position = [10.0, 40.0];
        w.tick();
        assert!(w.buildings[0].awake);
        assert_eq!(
            w.buildings[0].noticed,
            [Piece::Regiment(0), Piece::Regiment(1)]
        );
        assert_eq!(w.buildings[0].target, Some(Target::Unit(0)));
        assert_eq!(w.units[0].health, 90.0);
    }

    #[test]
    fn defenders_wake_for_what_the_map_placed_coming_back() {
        let mut w = sentry(&["infantry"]);
        run(&mut w, 1.0);
        w.units[0].position = [40.0, 40.0];
        run(&mut w, 1.0);
        assert!(!w.buildings[0].awake);
        w.units[0].position = [10.0, 10.0];
        w.tick();
        assert!(w.buildings[0].awake);
        assert_eq!(w.units[0].health, 90.0);
    }

    #[test]
    fn defenders_wake_for_what_changes_sides_near_them() {
        let mut w = sentry(&["infantry"]);
        w.regiments[0].owner = 2;
        run(&mut w, 1.0);
        assert!(w.buildings[0].noticed.is_empty());
        w.regiments[0].owner = 1;
        w.tick();
        assert!(w.buildings[0].awake);
        assert_eq!(w.units[0].health, 90.0);
    }

    #[test]
    fn defenders_notice_what_becomes_a_target_only_once_it_steps() {
        let mut w = sentry(&["infantry"]);
        let air = vec![Id::from("air")];
        w.units[0].fighter.as_mut().unwrap().flags = air;
        run(&mut w, 1.0);
        w.units[0].fighter.as_mut().unwrap().flags = vec![Id::from("infantry")];
        run(&mut w, 1.0);
        assert!(w.buildings[0].noticed.is_empty());
        assert!(!w.buildings[0].awake);
        w.units[0].position = [10.0, 12.0];
        w.tick();
        assert_eq!(w.buildings[0].noticed, [Piece::Regiment(0)]);
        assert!(w.buildings[0].awake);
    }

    #[test]
    fn captured_defenders_notice_their_new_enemies_only_once_they_step() {
        let mut w = sentry(&["infantry"]);
        w.regiments[0].owner = 2;
        run(&mut w, 1.0);
        w.buildings[0].owner = 1;
        run(&mut w, 1.0);
        assert!(w.buildings[0].noticed.is_empty());
        assert!(!w.buildings[0].awake);
        w.units[0].position = [10.0, 12.0];
        w.tick();
        assert_eq!(w.buildings[0].noticed, [Piece::Regiment(0)]);
        assert!(w.buildings[0].awake);
    }

    #[test]
    fn defenders_saved_awake_fire_on_what_the_map_placed() {
        let w = sentry(&["infantry"]);
        let mut pillbox = building(21, 2, [10.0, 24.0]);
        pillbox.fighter = w.buildings[0].fighter.clone();
        pillbox.awake = true;
        let mut w = battle(&[1], vec![soldier(Vec::new())], at_war());
        w = with_buildings(w, &[pillbox]);
        w.tick();
        assert!(w.buildings[0].awake);
        assert_eq!(w.units[0].health, 90.0);
    }

    #[test]
    fn defenders_wake_only_for_what_their_weapons_can_aim_at() {
        let mut w = sentry(&["air"]);
        run(&mut w, 1.0);
        w.units[1].position = [10.0, 40.0];
        run(&mut w, 2.0);
        assert_eq!(w.buildings[0].noticed, [Piece::Regiment(0)]);
        assert!(!w.buildings[0].awake);
        assert_eq!(w.units[0].health, 100.0);
    }

    #[test]
    fn defending_turrets_turn_to_their_target_before_firing() {
        // Pointing 90° off the soldier, it takes a second to turn.
        let mut w = turreted(90.0, 360.0);
        run(&mut w, 0.5);
        assert!((w.buildings[0].turrets[0].yaw - 45f32.to_radians()).abs() < 1e-4);
        assert_eq!(w.buildings[0].target, Some(Target::Unit(0)));
        run(&mut w, 0.4);
        assert_eq!(w.units[0].health, 100.0);
        run(&mut w, 0.2);
        assert_eq!(w.units[0].health, 90.0);
        assert_eq!(w.buildings[0].turrets[0].yaw, 0.0);
    }

    #[test]
    fn defending_turrets_fire_only_within_their_arc() {
        // 35° either side of its start, to its left: the soldier's ahead.
        let mut w = turreted(90.0, 70.0);
        run(&mut w, 3.0);
        assert!(w.buildings[0].noticed.is_empty());
        assert_eq!(w.units[0].health, 100.0);
        assert_eq!(w.buildings[0].turrets[0].yaw, 90f32.to_radians());
        // Facing -y, its left is +x.
        w.units[0].position = [20.0, 22.0];
        run(&mut w, 1.0);
        assert_eq!(w.buildings[0].noticed, [Piece::Regiment(0)]);
        assert!(w.units[0].health < 100.0);
        // Back ahead of it, the soldier's out of the arc and left be.
        w.units[0].position = [10.0, 14.0];
        let health = w.units[0].health;
        run(&mut w, 10.0);
        assert_eq!(w.units[0].health, health);
    }

    #[test]
    fn defending_turrets_turn_back_once_idle_long_enough() {
        let mut w = turreted(90.0, 360.0);
        w.buildings[0].fighter.turrets[0].reset = Some(1.0);
        run(&mut w, 1.1);
        assert_eq!(w.units[0].health, 90.0);
        // Out of reach it's left with nothing to turn to, a second after it
        // last turned to fire.
        w.units[0].position = [10.0, 0.0];
        run(&mut w, 0.5);
        assert_eq!(w.buildings[0].turrets[0].yaw, 0.0);
        run(&mut w, 2.0);
        assert_eq!(w.buildings[0].turrets[0].yaw, 90f32.to_radians());
    }

    #[test]
    fn turrets_hold_while_their_weapon_recharges() {
        let mut w = turreted(0.0, 360.0);
        w.buildings[0].fighter.weapons[0].recharge = 3.0;
        w.tick();
        assert_eq!(w.units[0].health, 90.0);
        // Moved to its left (and walking back), it waits for the rifle,
        // then turns.
        w.units[0].position = [18.0, 24.0];
        run(&mut w, 2.0);
        assert_eq!(w.buildings[0].turrets[0].yaw, 0.0);
        run(&mut w, 1.1);
        assert!(w.buildings[0].turrets[0].yaw > 0.0);
        assert_eq!(w.units[0].health, 90.0);
        run(&mut w, 1.0);
        assert_eq!(w.units[0].health, 80.0);
    }

    #[test]
    fn weapons_sharing_a_turret_each_turn_it() {
        // Two rifles on one turret turn it 90° in half a second, not one.
        let mut w = turreted(90.0, 360.0);
        let rifle = w.buildings[0].fighter.weapons[0].clone();
        w.buildings[0].fighter.weapons.push(rifle);
        w.buildings[0].reloads.push(0.0);
        run(&mut w, 0.4);
        assert_eq!(w.units[0].health, 100.0);
        run(&mut w, 0.2);
        assert_eq!(w.buildings[0].turrets[0].yaw, 0.0);
        assert_eq!(w.units[0].health, 80.0);
    }

    #[test]
    fn turrets_that_never_turn_fire_only_in_line() {
        let mut w = turreted(90.0, 360.0);
        w.buildings[0].fighter.turrets[0].turn_rate = 0.0;
        run(&mut w, 3.0);
        assert_eq!(w.units[0].health, 100.0);
        assert_eq!(w.buildings[0].turrets[0].yaw, 90f32.to_radians());
        // Straight ahead of one pointing ahead.
        let mut w = turreted(0.0, 360.0);
        w.buildings[0].fighter.turrets[0].turn_rate = 0.0;
        w.tick();
        assert_eq!(w.units[0].health, 90.0);
    }

    #[test]
    fn turrets_aim_from_their_offset() {
        let turret = Turret {
            turn_rate: 1.0,
            sweep: TAU,
            start: 0.0,
            reset: None,
            offset: [0.0, 2.0],
        };
        let mut w = siege(&[building(20, 2, [0.0, 0.0])]);
        w.buildings[0].facing = [0.0, 1.0];
        let b = &w.buildings[0];
        // Two to the left of the building's middle is -x when facing +y.
        assert_eq!(b.yaw_to(&turret, [-2.0, 5.0]), Some(0.0));
        assert_eq!(b.yaw_to(&turret, [-2.0, 0.0]), None);
        let right = b.yaw_to(&turret, [0.0, 0.0]).unwrap();
        assert!((right + PI / 2.0).abs() < 1e-6);
    }

    #[test]
    fn ranges_run_between_units_edges() {
        // 14 apart, out of the rifles' 12 until each reaches 1 from its
        // middle.
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![rifle(12.0, 30.0)]), soldier(Vec::new())],
            at_war(),
        );
        w.units[1].position = [10.0, 24.0];
        w.tick();
        assert_eq!(w.units[1].health, 100.0);
        for u in &mut w.units {
            u.fighter.as_mut().unwrap().target = Shape::Circle { radius: 1.0 };
        }
        w.tick();
        assert_eq!(w.units[1].health, 70.0);
        // Too near once the gap is no more than the least it fires at.
        let weapon = Weapon {
            min_range: 12.0,
            ..rifle(20.0, 30.0)
        };
        let (s, t) = (Target::Unit(0), Target::Unit(1));
        assert!(!w.can_hit(s, &weapon, t));
        w.units[1].position = [10.0, 24.5];
        assert!(w.can_hit(s, &weapon, t));
    }

    #[test]
    fn ranges_take_what_meets_a_rectangle_as_the_circle_round_it() {
        // A house 6 square, its edge 12 from a soldier's middle.
        let mut w = siege(&[building(20, 2, [10.0, 25.0])]);
        let (s, t) = (Target::Unit(0), Target::Building(0));
        assert_eq!(w.gap(s, t), Some(12.0));
        // The soldier as a rectangle 2 by 4: the circle through its
        // corners, whichever fires.
        w.units[0].fighter.as_mut().unwrap().target = Shape::Rect {
            width: 2.0,
            height: 4.0,
        };
        let round = libm::hypotf(2.0, 4.0) / 2.0;
        assert!((w.gap(s, t).unwrap() - (12.0 - round)).abs() < 1e-5);
        assert_eq!(w.gap(s, t), w.gap(t, s));
        // Standing on it is in range of everything.
        w.units[0].position = [10.0, 24.0];
        assert_eq!(w.gap(s, t), None);
        let weapon = Weapon {
            min_range: 5.0,
            ..rifle(1.0, 30.0)
        };
        assert!(w.can_hit(s, &weapon, t));
    }

    #[test]
    fn area_shots_hit_no_more_than_their_most_the_nearest_first() {
        let shell = Weapon {
            area: 6.0,
            max_targets: 2,
            ..rifle(30.0, 40.0)
        };
        let mut w = battle(
            &[1, 2, 2, 2],
            vec![
                soldier(vec![shell]),
                soldier(Vec::new()),
                soldier(Vec::new()),
                soldier(Vec::new()),
            ],
            at_war(),
        );
        w.units[1].position = [10.0, 30.0];
        w.units[2].position = [14.0, 30.0];
        w.units[3].position = [7.0, 30.0];
        w.units[0].target = Some(Target::Unit(1));
        w.tick();
        let health: Vec<f32> = w.units[1..].iter().map(|u| u.health).collect();
        assert_eq!(health, [60.0, 100.0, 60.0]);
    }

    #[test]
    fn ranges_keep_a_rectangle_before_a_bigger_square() {
        let mut hall = building(20, 2, [10.0, 10.0]);
        hall.fighter.target_square = true;
        let mut shed = building(21, 2, [10.0, 25.0]);
        shed.fighter.target = Shape::Rect {
            width: 2.0,
            height: 4.0,
        };
        let w = siege(&[hall, shed]);
        // To the shed's wall 13 off, less the circle round the hall.
        let (hall, shed) = (Target::Building(0), Target::Building(1));
        let round = libm::hypotf(6.0, 6.0) / 2.0;
        assert!((w.gap(hall, shed).unwrap() - (13.0 - round)).abs() < 1e-5);
        assert_eq!(w.gap(hall, shed), w.gap(shed, hall));
    }

    #[test]
    fn regiments_answer_fire_only_from_nearer_than_their_operational_range() {
        let mut w = battle(
            &[1, 2],
            vec![soldier(Vec::new()), soldier(vec![rifle(40.0, 10.0)])],
            at_war(),
        );
        w.units[1].position = [10.0, 44.0];
        w.tick();
        assert!(w.units[0].health < 100.0);
        assert!(w.regiments[0].engaged.is_empty());
        w.units[1].position = [10.0, 43.9];
        run(&mut w, 2.0);
        assert_eq!(w.regiments[0].engaged, [Piece::Regiment(1)]);
    }

    #[test]
    fn area_shots_reach_the_circle_round_a_targets_soft_shape() {
        let shell = Weapon {
            area: 6.0,
            ..rifle(30.0, 40.0)
        };
        // A bystander 7 from where the shell lands.
        let mut w = battle(
            &[1, 2, 2],
            vec![
                soldier(vec![shell]),
                soldier(Vec::new()),
                soldier(Vec::new()),
            ],
            at_war(),
        );
        w.units[1].position = [10.0, 30.0];
        w.units[2].position = [17.0, 30.0];
        w.units[0].target = Some(Target::Unit(1));
        w.tick();
        assert_eq!((w.units[1].health, w.units[2].health), (60.0, 100.0));
        w.units[2].fighter.as_mut().unwrap().soft = Shape::Rect {
            width: 2.0,
            height: 2.0,
        };
        run(&mut w, 2.0);
        assert_eq!((w.units[1].health, w.units[2].health), (20.0, 60.0));
    }

    #[test]
    fn range_reaches_to_a_buildings_edge() {
        // Its middle is 14 off, out of the rifle's 12, but its edge 11.
        let mut w = siege(&[building(20, 2, [10.0, 24.0])]);
        w.tick();
        assert_eq!(w.units[0].target, Some(Target::Building(0)));
        assert_eq!(w.buildings[0].health, 70.0);
    }

    #[test]
    fn enemy_damage_keeps_a_building_under_siege_for_a_while() {
        let mut w = siege(&[building(20, 2, [10.0, 24.0])]);
        w.tick();
        assert!(w.buildings[0].besieged.is_some());
        // Out of range from then on.
        w.units[0].position = [10.0, -20.0];
        run(&mut w, 6.5);
        assert!(w.buildings[0].besieged.is_some());
        run(&mut w, 1.5);
        assert_eq!(w.buildings[0].health, 70.0);
        assert!(w.buildings[0].besieged.is_none());
    }

    #[test]
    fn armed_enemies_come_before_buildings() {
        // The house is nearer, but the enemy soldier can fire back.
        let mut w = battle(
            &[1, 2],
            vec![
                soldier(vec![rifle(12.0, 30.0)]),
                soldier(vec![rifle(6.0, 1.0)]),
            ],
            at_war(),
        );
        w.add_buildings(&[building(20, 2, [16.0, 10.0])]);
        w.tick();
        assert_eq!(w.units[0].target, Some(Target::Unit(1)));
        assert_eq!(w.buildings[0].health, 100.0);
        run(&mut w, 10.0);
        assert!(!w.units[1].alive());
        assert_eq!(w.units[0].target, Some(Target::Building(0)));
    }
}
