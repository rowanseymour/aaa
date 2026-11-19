//! Weapons: what they fire on, turrets, projectiles and the damage they
//! do.

use tgmap::{
    CombatEffect, Damage, Does, Fighter, Id, Obstacle, Shape, TerrainEffect, Tile, Turret, Weapon,
    WeaponHit,
};

use crate::abilities::Rating;
use crate::value::MULTIPLY_FLOOR;
use crate::{
    Footing, LANDINGS_KEPT, Landing, Order, Piece, Projectile, Relation, State, TICK, Target,
    TurretAim, World, bounding_radius, distance, supply, targets::User, wrap,
};

/// The most seconds a unit waiting to use an ability goes before deciding
/// again.
const WAIT_SECONDS: f32 = 0.6;

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

/// A hit an enemy dealt something, as it's remembered for crediting the
/// kill.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Struck {
    /// What fired it, unless it was a special operation's.
    pub from: Option<Target>,
    /// The side that dealt it.
    pub by: u32,
    /// The tick it landed on.
    pub at: u64,
    /// Its main damage's [`tgmap::Death::filter`].
    pub filter: i32,
    /// The health it took.
    pub damage: f32,
}

crate::hash::digest!(Struck {
    from,
    by,
    at,
    filter,
    damage
});

/// Damage a shot does to what it lands on, for the side that fired it.
pub(crate) struct Hit {
    target: Target,
    amount: f32,
    /// What fired it, unless it's a special operation's.
    from: Option<Target>,
    by: u32,
    /// For bombardment, its weapon's seconds between shots.
    bombard: Option<f32>,
    /// The morale it takes from the organization of the unit it lands on.
    morale: f32,
    /// Its damage's kind.
    kind: Option<Id>,
    /// It's what the weapon's user did to itself.
    on_user: bool,
}

impl Hit {
    #[cfg(test)]
    pub(crate) fn target(&self) -> Target {
        self.target
    }

    #[cfg(test)]
    pub(crate) fn amount(&self) -> f32 {
        self.amount
    }

    /// A hit of `by`'s special operation: the side's as a whole, so neither
    /// bombardment nor drawing fire.
    pub(crate) fn strike(target: Target, amount: f32, by: u32, kind: Option<Id>) -> Hit {
        Hit {
            target,
            amount,
            from: None,
            by,
            bombard: None,
            morale: 0.0,
            kind,
            on_user: false,
        }
    }
}

/// The fraction of an area shot's damage that reaches `d` from where it
/// lands: full within [`Weapon::full_within`], falling
/// evenly to [`Weapon::edge_damage`] at the edge.
fn falloff(weapon: &Weapon, d: f32) -> f32 {
    attenuated([weapon.full_within, weapon.area], weapon.edge_damage, d)
}

/// The fraction of damage that reaches `d` from the middle of an area
/// `[full, area]`: full within `full`, falling evenly to `edge` at `area`.
pub(crate) fn attenuated([full, area]: [f32; 2], edge: f32, d: f32) -> f32 {
    let full = full.min(area);
    if d <= full || area <= full {
        return 1.0;
    }
    let along = (d - full) / (area - full);
    1.0 + (edge - 1.0) * along
}

/// The share of damage of `kind` that resistance takes off `target`, with
/// what the terrain properties it holds, `ground`, add.
pub(crate) fn resistance(target: &Fighter, ground: &[&TerrainEffect], kind: &Id) -> f32 {
    let grounds = ground.iter().flat_map(|e| &e.resists);
    target.resists.get(kind).copied().unwrap_or(0.0)
        + (grounds.filter(|(k, _)| k == kind))
            .map(|(_, amount)| amount)
            .sum::<f32>()
}

/// What a hit's main damage gets beyond its weapon's own: from what fired
/// it and from what it lands on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Blow {
    /// The attack efficiency it's dealt with.
    pub efficiency: f32,
    /// Added to its main damage.
    pub attack: f32,
    /// Added to its target's defense.
    pub defense: f32,
    /// It ignores defense.
    pub vorpal: bool,
}

impl Default for Blow {
    fn default() -> Self {
        Blow {
            efficiency: 1.0,
            attack: 0.0,
            defense: 0.0,
            vorpal: false,
        }
    }
}

/// Health main damage `d` takes from `target`, scaled by `scale` and
/// `blow`'s efficiency: defence comes off first, then resistance takes its
/// share, and every hit does at least 1. `ground` is what the terrain
/// properties the target holds add to both, and `blow` what else adds to
/// the damage and to defense.
pub(crate) fn damage(
    d: &Damage,
    target: &Fighter,
    ground: &[&TerrainEffect],
    scale: f32,
    blow: Blow,
) -> f32 {
    let defense = target.defense + blow.defense + ground.iter().map(|e| e.defense).sum::<f32>();
    let defense = if blow.vorpal { 0.0 } else { defense.max(0.0) };
    let resist = resistance(target, ground, &d.kind);
    // What lowers it takes it no lower than 1, or its own if that's less.
    let amount = (d.amount + blow.attack).max(d.amount.min(1.0)).max(0.0);
    ((blow.efficiency * amount * scale - defense) * (1.0 - resist)).max(1.0)
}

/// Health additive damage `d` takes from `target` standing on `ground`:
/// only resistance lessens it.
pub(crate) fn added(d: &Damage, target: &Fighter, ground: &[&TerrainEffect]) -> f32 {
    d.amount * (1.0 - resistance(target, ground, &d.kind))
}

/// What `effect` does to `f`, standing on `ground`, with `scale` of falloff
/// and `blow` for main damage: the health it takes, the morale it takes
/// from its organization, and its damage's kind. Only main damage and
/// morale lessen away from an area's middle.
fn effect_does(
    effect: &WeaponHit,
    f: &Fighter,
    ground: &[&TerrainEffect],
    scale: f32,
    blow: Blow,
) -> (f32, f32, Option<Id>) {
    match &effect.does {
        Does::Main(d) => (damage(d, f, ground, scale, blow), 0.0, Some(d.kind.clone())),
        Does::Add(d) => (added(d, f, ground), 0.0, Some(d.kind.clone())),
        Does::Morale(amount) => (0.0, amount * scale, None),
    }
}

impl World {
    /// What the properties `t` holds add to its fighting.
    pub(crate) fn effect(&self, t: Target) -> CombatEffect {
        match t {
            Target::Unit(u) => self.boost(u),
            Target::Building(b) => self.building_boost(b),
        }
    }

    /// What the properties the building at `b` holds add to its fighting:
    /// its template's own, its side's technologies on its kind and those
    /// it holds for a time.
    pub(crate) fn building_boost(&self, b: usize) -> CombatEffect {
        let building = &self.buildings[b];
        self.held_effect(
            building.owner,
            &building.template,
            &building.properties,
            &[],
        )
    }

    /// What a hit `from`'s `weapon` fires, or a special operation's
    /// without either, gets beyond the weapon's own landing on `to`.
    pub(crate) fn blow(&self, from: Option<Target>, weapon: Option<&Weapon>, to: Target) -> Blow {
        let by = from.map(|f| self.effect(f)).unwrap_or_default();
        let attack = match weapon {
            Some(w) if from.is_some() => {
                let reach = if w.projectile.is_some() {
                    by.ranged
                } else {
                    by.melee
                };
                by.attack + reach
            }
            _ => 0.0,
        };
        let spell = weapon.is_some_and(|w| w.spell);
        Blow {
            efficiency: if spell { 1.0 } else { self.efficiency(from) },
            attack,
            defense: self.effect(to).defense,
            vorpal: weapon.is_some_and(|w| w.vorpal),
        }
    }

    /// What `t`'s properties multiply its weapons' rate of fire by.
    pub(crate) fn fire_rate(&self, t: Target) -> f32 {
        (1.0 + self.effect(t).recharge).max(MULTIPLY_FLOOR)
    }

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

    /// A target's defence as damage finds it now, and its template's own,
    /// for one with a body.
    pub fn defense(&self, t: Target) -> Option<(f32, f32)> {
        let plain = match t {
            Target::Unit(u) => self.units[u].fighter.as_ref()?.defense,
            Target::Building(b) => self.buildings[b].fighter.defense,
        };
        let ground = self
            .terrain_effects(t)
            .iter()
            .map(|e| e.defense)
            .sum::<f32>();
        let level = self.effect(t).defense;
        Some((plain + ground + level, plain))
    }

    pub(crate) fn owner(&self, t: Target) -> u32 {
        match t {
            Target::Unit(u) => self.organizations[self.units[u].organization].owner,
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

    /// Has each unit act on the ability it chooses and each building fire
    /// every weapon that's ready at what it can hit, then takes the dead
    /// out of the fight, so that all hit each other at once. Returns, for
    /// each unit hit, its organization and what fired.
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
                // A site has no weapons until it's finished.
                Target::Building(b) if self.buildings[b].constructing => false,
                Target::Building(b) => !defends(b) || self.buildings[b].awake,
                Target::Unit(_) => true,
            })
            .collect();
        for s in shooters {
            if let Target::Unit(u) = s {
                self.act(u, &mut hits, &mut launched);
                continue;
            }
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
            let (mut fired_ago, mut target, mut shot, moving) = match s {
                Target::Unit(u) => {
                    let u = &self.units[u];
                    (u.fired_ago.clone(), u.target, u.shot, u.moving())
                }
                Target::Building(b) => {
                    let b = &self.buildings[b];
                    (b.fired_ago.clone(), b.target, b.shot, false)
                }
            };
            // Only a weapon that's fired since its shooter last stood still
            // needs the rate: one that hasn't is ready at any.
            let mut rate = None;
            let by = self.owner(s);
            // Nothing fires while routing; reloading goes on.
            let held = match s {
                Target::Unit(u) => self.organizations[self.units[u].organization].routing(),
                Target::Building(_) => false,
            };
            for aim in &mut turrets {
                aim.idle += TICK;
            }
            for (w, weapon) in fighter.weapons.iter().enumerate() {
                // The rate is read as the time is checked, so a change to it
                // shortens or lengthens a recharge under way.
                fired_ago[w] += TICK;
                if held || moving && !weapon.turret {
                    continue;
                }
                let ready = fired_ago[w].is_infinite() || {
                    let rate = *rate.get_or_insert_with(|| self.fire_rate(s));
                    fired_ago[w] * rate >= weapon.recharge - 1e-4
                };
                if !ready {
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
                fired_ago[w] = 0.0;
                self.use_on_user(weapon, (s, by), &mut hits);
                match weapon.flight {
                    Some(flight) => launched.push(self.launch(s, by, w, weapon, flight, t)),
                    None => {
                        let bombard = self.bombards(s, weapon);
                        let at = self.impact(s, weapon, self.position(t));
                        self.strike(weapon, at, t, (s, by), bombard, &mut hits)
                    }
                }
            }
            match s {
                Target::Unit(u) => {
                    let u = &mut self.units[u];
                    (u.fired_ago, u.target, u.shot) = (fired_ago, target, shot);
                }
                Target::Building(b) => {
                    let b = &mut self.buildings[b];
                    for (aim, turret) in turrets.iter_mut().zip(&b.fighter.turrets) {
                        if turret.reset.is_some_and(|r| aim.idle > r) {
                            aim.turn(turret, turret.start);
                        }
                    }
                    (b.fired_ago, b.target, b.shot, b.turrets) = (fired_ago, target, shot, turrets);
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
        self.land_strikes(&mut hits);
        let mut fired_on = Vec::new();
        self.land_all(hits, &mut fired_on);
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

    /// Has the unit at `u`, unless it's routing or not yet due to decide,
    /// choose what to take on and with which weapon, and act on it: use
    /// the weapon, hold until it's ready or for [`WAIT_SECONDS`] if that's
    /// sooner, or go after its target.
    fn act(&mut self, u: usize, hits: &mut Vec<Hit>, launched: &mut Vec<Projectile>) {
        let unit = &mut self.units[u];
        for ago in unit.fired_ago.iter_mut().chain(&mut unit.mended_ago) {
            *ago += TICK;
        }
        let r = self.units[u].organization;
        if self.organizations[r].routing() {
            // What hit it or it ran into while routing is behind it.
            let unit = &mut self.units[u];
            (unit.decided, unit.met) = (self.ticks, None);
            return;
        }
        if self.ticks < self.units[u].busy {
            return;
        }
        let mut rng = self.rng;
        let decided = self.decide_attack(u, &mut rng);
        self.rng = rng;
        let unit = &mut self.units[u];
        unit.decided = self.ticks;
        unit.met = None;
        let Some((t, pick)) = decided else {
            unit.target = None;
            self.mend(u);
            return;
        };
        unit.target = Some(t);
        if pick.rating == Rating::MoveIntoRange {
            unit.closing = Some(pick.range);
            return;
        }
        if unit.closing.take().is_some() {
            unit.halt();
        }
        if pick.rating == Rating::Wait {
            let most = (WAIT_SECONDS / TICK).round() as u64;
            self.units[u].busy = self.ticks + self.recharged_in(u, pick.weapon, most);
            return;
        }
        let fighter = unit.fighter.as_ref().expect("units that pick can fight");
        let weapon = fighter.weapons[pick.weapon].clone();
        unit.shot = Some((self.ticks, pick.weapon));
        unit.fired_ago[pick.weapon] = 0.0;
        unit.busy = self.ticks + (weapon.choice.lasts / TICK).round() as u64;
        let s = Target::Unit(u);
        let by = self.owner(s);
        self.use_on_user(&weapon, (s, by), hits);
        match weapon.flight {
            Some(flight) => launched.push(self.launch(s, by, pick.weapon, &weapon, flight, t)),
            None => {
                let bombard = self.bombards(s, &weapon);
                let at = self.impact(s, &weapon, self.position(t));
                self.strike(&weapon, at, t, (s, by), bombard, hits)
            }
        }
    }

    /// Where the area of `s`'s `weapon` is centred for a shot landing at
    /// `at`: round `s` where it stands for one centred on its user.
    fn impact(&self, s: Target, weapon: &Weapon, at: [f32; 2]) -> [f32; 2] {
        match weapon.centred_on_user && weapon.area > 0.0 {
            true => self.position(s),
            false => at,
        }
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
        flight: tgmap::Flight,
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
            at: self.impact(s, weapon, [0, 1].map(|i| aim[i] + velocity[i] * seconds)),
            lands: self.ticks + (seconds / TICK - 1e-4).ceil().max(0.0) as u64,
            bombard: self.bombards(s, weapon),
        }
    }

    /// Takes a hit's damage off what it lands on. A building whose health
    /// runs out is captured by the hit's owner if it can be, else destroyed
    /// if it can be, else disabled.
    fn land_all(&mut self, hits: Vec<Hit>, fired_on: &mut Vec<(usize, Target)>) {
        // A unit one effect of a use kills still takes the use's later
        // effects, the last of them its last hit, before its death counts.
        let mut dying: Option<(usize, Option<Target>, u32)> = None;
        for hit in hits {
            let (from, by) = (hit.from, hit.by);
            let of_use = move |u| (u, from, by);
            let follows = matches!(hit.target, Target::Unit(u) if dying == Some(of_use(u)));
            if !follows && let Some((u, ..)) = dying.take() {
                self.mourn(u);
                self.lost_member(u);
            }
            if let Target::Unit(u) = hit.target {
                let r = self.units[u].organization;
                let alive = self.units[u].alive();
                match (hit.bombard, hit.from) {
                    // Bombardment keeps what it hurts out of supply rather
                    // than drawing its fire.
                    (Some(recharge), Some(from)) if alive => self.shell(r, from, recharge),
                    (None, Some(from)) => fired_on.push((r, from)),
                    _ => {}
                }
            }
            let target = hit.target;
            if self.strike_down(hit, follows)
                && let Target::Unit(u) = target
            {
                dying = Some(of_use(u));
            }
        }
        if let Some((u, ..)) = dying {
            self.mourn(u);
            self.lost_member(u);
        }
    }

    /// Lands a hit, a unit it kills counting as dead at once.
    #[cfg(test)]
    fn land(&mut self, hit: Hit) {
        self.land_all(vec![hit], &mut Vec::new());
    }

    /// Takes a hit's damage off what it lands on, as [`World::land`] does,
    /// but leaves a unit it kills to be mourned by the caller: returns
    /// whether it killed one. One that `follows` a hit of the same use
    /// that killed its unit still counts as that unit's last hit.
    fn strike_down(&mut self, hit: Hit, follows: bool) -> bool {
        let ticks = self.ticks;
        let window = (self.rules.siege_seconds / TICK).round() as u64;
        let filters = &self.rules.damage_filters;
        let struck = Struck {
            from: hit.from,
            by: hit.by,
            at: ticks,
            filter: (hit.kind.as_ref())
                .and_then(|k| filters.get(k).copied())
                .unwrap_or(tgmap::ANY_DAMAGE),
            damage: hit.amount,
        };
        match hit.target {
            Target::Unit(u) => {
                let owner = self.organizations[self.units[u].organization].owner;
                let enemy = self.sides.relation(hit.by, owner) == Relation::Enemy;
                let harmful = enemy && hit.amount > 0.0;
                let unit = &mut self.units[u];
                if !unit.alive() && !follows {
                    return false;
                }
                if harmful {
                    besiege(&mut unit.besieged, ticks + window);
                    unit.struck = Some(struck);
                }
                let dead = !follows && {
                    unit.health -= hit.amount;
                    unit.health <= 0.0
                };
                if dead {
                    unit.die(ticks);
                }
                let r = self.units[u].organization;
                if harmful {
                    self.organizations[r].struck = Some(struck);
                }
                if enemy || hit.on_user {
                    self.lose_morale(r, hit.morale, true);
                }
                dead
            }
            Target::Building(b) => {
                let building = &mut self.buildings[b];
                // Nothing a side fires hurts its own, such as what it took
                // over earlier in the tick; its special operations do.
                let own = building.owner == hit.by && hit.from.is_some();
                if !building.standing() || own {
                    return false;
                }
                if self.sides.relation(hit.by, building.owner) == Relation::Enemy
                    && hit.amount > 0.0
                {
                    building.struck = Some(struck);
                    // A bombarder holds it for as long as it keeps a
                    // organization out of supply, if that's longer.
                    let held = hit.bombard.map_or(0.0, |recharge| {
                        self.rules.supply_rules.bombard_timeout + recharge
                    });
                    let held = (held / TICK).round() as u64;
                    besiege(&mut building.besieged, ticks + window.max(held));
                }
                building.health -= hit.amount;
                if building.health <= 0.0 {
                    match hit.from {
                        Some(from) => {
                            let owner = self.buildings[b].owner;
                            self.fall(b, Some(hit.by));
                            if self.buildings[b].owner != owner {
                                self.credit_fall(b, from, true);
                            }
                        }
                        None => self.fall_unclaimed(b),
                    }
                }
                false
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
    /// A unit whose organization is pursuing an enemy fires on that enemy
    /// first whenever it can.
    fn aim(&self, s: Target, weapon: &Weapon, last: Option<Target>) -> Option<Target> {
        if let Target::Building(b) = s
            && self.buildings[b].fighter.notice.is_some()
        {
            return self.defend(b, weapon, last);
        }
        let from = self.position(s);
        let quarry = match s {
            Target::Unit(u) => match self.organizations[self.units[u].organization].order {
                Order::Pursue(e) => Some(e),
                _ => None,
            },
            Target::Building(_) => None,
        };
        let of_quarry = |t: Target| match (quarry, t) {
            (Some(e), Target::Unit(v)) => self.units[v].organization == e,
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
    /// whole organization. Stepping onto another of the zone's tiles, or
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
                        .any(|w| self.can_target(s, w, t) && self.in_arc(s, w, t))
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

    /// The organization or building a target is, or is in.
    pub(crate) fn piece(&self, t: Target) -> Piece {
        match t {
            Target::Unit(u) => Piece::Organization(self.units[u].organization),
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
                && self.in_arc(s, weapon, t)
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

    /// Whether `s`'s turret for `weapon`, if it has one, can be aimed at
    /// `t`'s centre.
    pub(crate) fn in_arc(&self, s: Target, weapon: &Weapon, t: Target) -> bool {
        let (position, facing) = match s {
            Target::Unit(u) => (self.units[u].position, self.units[u].facing),
            Target::Building(b) => (self.buildings[b].position, self.buildings[b].facing),
        };
        let turrets = self.fighter(s).map(|f| &f.turrets[..]).unwrap_or_default();
        (weapon.aimed_by.and_then(|i| turrets.get(i)))
            .is_none_or(|turret| crate::in_arc(position, facing, turret, self.position(t)))
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
            let organization = &self.organizations[unit.organization];
            // A unit in no organization is one of its own template.
            let lone = unit.template == organization.template;
            if organization.aggressive() || armed && lone {
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
    pub(crate) fn gap(&self, s: Target, t: Target) -> Option<f32> {
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
        t != s && self.matches(s, &weapon.targets, t)
    }

    /// Whether what `t` is has the target types `targets` asks for, as
    /// `s` sees it, alive or not.
    pub fn matches(&self, s: Target, targets: &tgmap::Targets, t: Target) -> bool {
        self.fighter(t).is_some() && self.reaches(self.user(s), targets, t)
    }

    /// `s` as the user of what it fires.
    pub(crate) fn user(&self, s: Target) -> User {
        User {
            player: self.owner(s),
            thing: Some(s),
        }
    }

    /// What the effects of `weapon` that go to its user, `from` of `by`'s
    /// side, do to it as it's used: before anything else of the use lands,
    /// and as if from its own side.
    fn use_on_user(&self, weapon: &Weapon, (from, by): (Target, u32), hits: &mut Vec<Hit>) {
        let Some(f) = self.fighter(from) else {
            return;
        };
        let ground = self.terrain_effects(from);
        let blow = self.blow(Some(from), Some(weapon), from);
        for h in weapon.hits.iter().filter(|h| h.on_user) {
            let (amount, morale, kind) = effect_does(h, f, &ground, 1.0, blow);
            hits.push(Hit {
                target: from,
                amount,
                from: None,
                by,
                bombard: None,
                morale,
                kind,
                on_user: true,
            });
        }
    }

    /// Records what a shot fired by `from` of `by`'s side, as bombardment
    /// or not, landing at `at` and aimed at `t`, does to what it hits: `t`,
    /// or for an area shot whatever around `at` the weapon fires on. Each
    /// of its effects that reaches a thing hit is a hit of its own, in the
    /// order the weapon lists them.
    fn strike(
        &self,
        weapon: &Weapon,
        at: [f32; 2],
        t: Target,
        (from, by): (Target, u32),
        bombard: bool,
        hits: &mut Vec<Hit>,
    ) {
        let user = User {
            player: by,
            thing: Some(from),
        };
        let mut reached = Vec::new();
        if weapon.area <= 0.0 {
            if self.fighter(t).is_some() {
                reached.push((0.0, t, 1.0));
            }
        } else {
            for target in self.targets() {
                let f = self.fighter(target).expect("targets can fight");
                // To the circle round its soft shape, however it's turned.
                let middle = distance(at, self.position(target));
                let d = (middle - bounding_radius(f.soft)).max(0.0);
                if d <= weapon.area && self.reaches(user, &weapon.targets, target) {
                    reached.push((middle, target, falloff(weapon, d)));
                }
            }
            // Those nearest by their middles, where there's a most it hits.
            if weapon.max_targets > 0 {
                reached.sort_by(|a, b| a.0.total_cmp(&b.0));
                reached.truncate(weapon.max_targets as usize);
            }
        }
        let bombard = bombard.then_some(weapon.recharge);
        for (_, target, scale) in reached {
            let f = self.fighter(target).expect("targets can fight");
            let ground = self.terrain_effects(target);
            let blow = self.blow(Some(from), Some(weapon), target);
            for h in weapon.hits.iter().filter(|h| !h.on_user) {
                if !self.reaches(user, &h.targets, target) {
                    continue;
                }
                let (amount, morale, kind) = effect_does(h, f, &ground, scale, blow);
                hits.push(Hit {
                    target,
                    amount,
                    from: Some(from),
                    by,
                    bombard,
                    morale,
                    kind,
                    on_user: false,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    use std::f32::consts::{PI, TAU};

    use crate::{NotYours, Piece, Target, World};
    use tgmap::CombatEffect;

    /// A player 2 pillbox at (10, 24), noticing within 20, whose rifle
    /// can't hit aircraft, and unarmed soldiers of two of player 1's
    /// organizations: the first in its zone, the second, of `flags`, far off.
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
        // It engages, then fires.
        w.tick();
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
            flight: Some(tgmap::Flight {
                kind: tgmap::FlightKind::Straight,
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
        // It engages, then fires.
        w.tick();
        w.tick();
        assert_eq!(w.units[0].shot, Some((1, 0)));
        assert_eq!((w.units[0].health, w.units[1].health), (100.0, 100.0));
        assert_eq!(w.flying.len(), 2);
        run(&mut w, 0.3);
        assert_eq!((w.units[0].health, w.units[1].health), (100.0, 100.0));
        w.tick();
        assert_eq!((w.units[0].health, w.units[1].health), (100.0, 70.0));
        let landed = &w.landings[0];
        assert_eq!(
            (landed.tick, landed.from, landed.target),
            (5, Target::Unit(0), Target::Unit(1))
        );
        run(&mut w, 0.6);
        assert_eq!(w.units[0].health, 70.0);
        assert_eq!(w.landings.len(), 2);
    }

    #[test]
    fn efficiency_scales_main_damage_and_vorpal_ignores_defense() {
        let mut tank = soldier(Vec::new());
        tank.defense = 6.0;
        let rifle = main_damage(&rifle(12.0, 12.0));
        let hit = |blow| damage(&rifle, &tank, &[], 1.0, blow);
        let halved = Blow {
            efficiency: 0.5,
            ..Default::default()
        };
        // 12 * 0.5 - 6 is nothing, so the least.
        assert_eq!(hit(halved), 1.0);
        let vorpal = Blow {
            vorpal: true,
            ..halved
        };
        assert_eq!(hit(vorpal), 6.0);
        // What lowers it takes it no lower than 1.
        let feeble = Blow {
            attack: -20.0,
            vorpal: true,
            ..Default::default()
        };
        assert_eq!(hit(feeble), 1.0);
    }

    /// Player 1's rifleman of a regiment hitting at half strength, whose
    /// kind is `sharp`, and player 2's unarmed one 10 off.
    fn sharpshooter(sharp: CombatEffect) -> World {
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![rifle(12.0, 10.0)]), soldier(Vec::new())],
            at_war(),
        );
        w.organizations[0].efficiency = 0.5;
        w.rules.kinds.insert("soldier".into(), kind(&["sharp"]));
        w.rules.combat_effects.insert("sharp".into(), sharp);
        w
    }

    #[test]
    fn melee_and_ranged_attack_raise_only_their_own_attacks() {
        let sharp = CombatEffect {
            attack: 1.0,
            melee: 5.0,
            ranged: 3.0,
            ..Default::default()
        };
        let w = sharpshooter(sharp);
        let mut gun = w.units[0].fighter.as_ref().unwrap().weapons[0].clone();
        let blow = |gun: &Weapon| w.blow(Some(Target::Unit(0)), Some(gun), Target::Unit(1));
        assert_eq!(blow(&gun).attack, 6.0);
        assert_eq!(blow(&gun).efficiency, 0.5);
        gun.projectile = Some(Id::from("bullet"));
        assert_eq!(blow(&gun).attack, 4.0);
        // A spell hits at full strength; a special operation adds nothing.
        gun.spell = true;
        assert_eq!(blow(&gun).efficiency, 1.0);
        assert_eq!(w.blow(None, None, Target::Unit(1)), Blow::default());
    }

    #[test]
    fn recharge_time_multipliers_change_the_rate_of_fire() {
        let shots = |recharge: f32| {
            let mut w = sharpshooter(CombatEffect {
                recharge,
                ..Default::default()
            });
            w.organizations[0].efficiency = 1.0;
            run(&mut w, 4.0);
            ((100.0 - w.units[1].health) / 10.0).round()
        };
        // Every 2s, then every 1s, then every 4s.
        assert_eq!(shots(0.0), 2.0);
        assert_eq!(shots(1.0), 4.0);
        assert_eq!(shots(-0.5), 1.0);
    }

    #[test]
    fn a_buildings_properties_raise_its_defense() {
        let mut w = siege(&[building(9, 2, [10.0, 20.0])]);
        w.rules.kinds.insert("house".into(), kind(&["walls"]));
        let walls = CombatEffect {
            defense: 3.0,
            ..Default::default()
        };
        w.rules.combat_effects.insert("walls".into(), walls);
        let b = Target::Building(0);
        assert_eq!(w.blow(Some(Target::Unit(0)), None, b).defense, 3.0);
        assert_eq!(w.defense(b), Some((3.0, 0.0)));
    }

    #[test]
    fn a_buildings_health_scales_with_its_properties_keeping_its_share() {
        let mut w = siege(&[building(9, 2, [40.0, 40.0])]);
        w.buildings[0].health = 50.0;
        let stout = CombatEffect {
            health: 0.5,
            ..Default::default()
        };
        w.rules.combat_effects.insert("stout".into(), stout);
        w.give(Piece::Building(0), "stout".into(), 1.0);
        w.tick();
        assert_eq!(w.building_max_health(0), 150.0);
        assert_eq!(w.buildings[0].health, 75.0);
        run(&mut w, 2.0);
        assert_eq!(w.building_max_health(0), 100.0);
        assert_eq!(w.buildings[0].health, 50.0);
    }

    #[test]
    fn units_fire_while_they_take_their_places_in_a_fight() {
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![rifle(12.0, 10.0)]), soldier(Vec::new())],
            at_war(),
        );
        // Its combat formation stands it 2 to the side.
        let r = &mut w.organizations[0];
        r.formations.push(tgmap::Formation {
            id: "wide".into(),
            offsets: vec![[2.0, 0.0]],
            aggressive: true,
        });
        r.combat = Some(1);
        w.tick();
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
        w.tick();
        assert!(w.units[0].moving());
        assert_eq!(w.units[1].health, 90.0);
    }

    #[test]
    fn weapons_hold_fire_on_friends_what_they_exclude_and_what_is_out_of_range() {
        let mut tank = soldier(vec![rifle(12.0, 30.0)]);
        tank.flags = vec![Id::from("tank")];
        // 1 and 3 are allies, 2 a tank the rifles can't hurt, so can't
        // engage, and 4 out of the rifles' range.
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
        assert!(
            (damage(
                &main_damage(&rifle(12.0, 12.0)),
                &tank,
                &[],
                1.0,
                Blow::default()
            ) - 2.4)
                .abs()
                < 1e-5
        );
        assert_eq!(
            damage(
                &main_damage(&rifle(12.0, 5.0)),
                &tank,
                &[],
                1.0,
                Blow::default()
            ),
            1.0
        );
        assert!(
            (damage(
                &main_damage(&rifle(12.0, 50.0)),
                &tank,
                &[],
                0.5,
                Blow::default()
            ) - 7.6)
                .abs()
                < 1e-5
        );
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
        // A friend 2 from where the shell lands, on an enemy within the
        // control range.
        let mut w = battle(
            &[1, 1, 2],
            vec![
                soldier(vec![shell()]),
                soldier(Vec::new()),
                soldier(Vec::new()),
            ],
            at_war(),
        );
        w.units[1].position = [10.0, 23.0];
        w.units[2].position = [10.0, 25.0];
        // It engages, then fires.
        w.tick();
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
        assert_eq!(w.buildings[0].noticed, [Piece::Organization(0)]);
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
        assert_eq!(w.buildings[0].noticed, [Piece::Organization(0)]);
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
            [Piece::Organization(0), Piece::Organization(1)]
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
        w.organizations[0].owner = 2;
        run(&mut w, 1.0);
        assert!(w.buildings[0].noticed.is_empty());
        w.organizations[0].owner = 1;
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
        assert_eq!(w.buildings[0].noticed, [Piece::Organization(0)]);
        assert!(w.buildings[0].awake);
    }

    #[test]
    fn captured_defenders_notice_their_new_enemies_only_once_they_step() {
        let mut w = sentry(&["infantry"]);
        w.organizations[0].owner = 2;
        run(&mut w, 1.0);
        w.buildings[0].owner = 1;
        run(&mut w, 1.0);
        assert!(w.buildings[0].noticed.is_empty());
        assert!(!w.buildings[0].awake);
        w.units[0].position = [10.0, 12.0];
        w.tick();
        assert_eq!(w.buildings[0].noticed, [Piece::Organization(0)]);
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
        assert_eq!(w.buildings[0].noticed, [Piece::Organization(0)]);
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
        assert_eq!(w.buildings[0].noticed, [Piece::Organization(0)]);
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
        w.buildings[0].fired_ago.push(f32::INFINITY);
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
        w.units[1].position = [10.0, 25.0];
        w.units[2].position = [14.0, 25.0];
        w.units[3].position = [7.0, 25.0];
        w.units[0].target = Some(Target::Unit(1));
        // It engages, then fires.
        w.tick();
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
    fn organizations_answer_fire_only_from_nearer_than_their_operational_range() {
        let mut w = battle(
            &[1, 2],
            vec![
                soldier(vec![rifle(12.0, 1.0)]),
                soldier(vec![rifle(40.0, 10.0)]),
            ],
            at_war(),
        );
        // The rifleman's regiment is engaged from further off.
        w.organizations[1].operational_range = 50.0;
        w.organizations[1].engaged = vec![Piece::Organization(0)];
        w.units[1].position = [10.0, 44.0];
        w.tick();
        assert!(w.units[0].health < 100.0);
        assert!(w.organizations[0].engaged.is_empty());
        w.units[1].position = [10.0, 43.9];
        run(&mut w, 2.0);
        assert_eq!(w.organizations[0].engaged, [Piece::Organization(1)]);
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
        w.units[1].position = [10.0, 25.0];
        w.units[2].position = [17.0, 25.0];
        w.units[0].target = Some(Target::Unit(1));
        // It engages, then fires.
        w.tick();
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
        // It engages, then fires.
        w.tick();
        w.tick();
        assert_eq!(w.units[0].target, Some(Target::Building(0)));
        assert_eq!(w.buildings[0].health, 70.0);
    }

    #[test]
    fn only_enemies_hits_take_morale() {
        let mut w = battle(
            &[1, 3],
            vec![soldier(Vec::new()), soldier(Vec::new())],
            at_war(),
        );
        w.rules.morale_values.insert("soldier".into(), 50.0);
        w.refigure_morale(0, true);
        let hit = |by| Hit {
            target: Target::Unit(0),
            amount: 1.0,
            from: Some(Target::Unit(1)),
            by,
            bombard: None,
            morale: 0.75,
            kind: None,
            on_user: false,
        };
        // Player 3 is player 1's ally.
        w.land(hit(3));
        assert_eq!(w.organizations[0].morale, 50.0);
        w.land(hit(2));
        assert_eq!(w.organizations[0].morale, 50.0 - 0.75);
    }

    #[test]
    fn enemy_damage_keeps_a_building_under_siege_for_a_while() {
        let mut w = siege(&[building(20, 2, [10.0, 24.0])]);
        // It engages, then fires.
        w.tick();
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
        // It engages, then fires.
        w.tick();
        w.tick();
        assert_eq!(w.units[0].target, Some(Target::Unit(1)));
        assert_eq!(w.buildings[0].health, 100.0);
        run(&mut w, 10.0);
        assert!(!w.units[1].alive());
        assert_eq!(w.units[0].target, Some(Target::Building(0)));
    }

    fn hit(targets: &str, on_user: bool, does: Does) -> WeaponHit {
        let targets = tgmap::Targets {
            required: (targets.split(' ').filter(|t| !t.is_empty()).map(Id::from)).collect(),
            excluded: Vec::new(),
        };
        WeaponHit {
            targets,
            on_user,
            does,
        }
    }

    fn of(kind: &str, amount: f32) -> Damage {
        Damage {
            kind: Id::from(kind),
            amount,
        }
    }

    /// Player 1's soldier firing `weapon`, and player 2's, 10 off, it's
    /// aimed at, with what each hit of a use does to what, the user's
    /// first.
    fn used(weapon: Weapon, flags: &[&str]) -> (World, Vec<(Target, f32)>) {
        let mut target = soldier(Vec::new());
        target.flags = flags.iter().map(|&f| Id::from(f)).collect();
        let w = battle(
            &[1, 2],
            vec![soldier(vec![weapon.clone()]), target],
            at_war(),
        );
        let (s, t) = (Target::Unit(0), Target::Unit(1));
        let mut hits = Vec::new();
        w.use_on_user(&weapon, (s, 1), &mut hits);
        let at = w.impact(s, &weapon, w.position(t));
        w.strike(&weapon, at, t, (s, 1), false, &mut hits);
        let done = hits.iter().map(|h| (h.target, h.amount)).collect();
        (w, done)
    }

    fn sniper() -> Weapon {
        let mut weapon = rifle(12.0, 10.0);
        weapon.targets = Default::default();
        weapon
            .hits
            .push(hit("infantry", false, Does::Add(of("gunfire", 250.0))));
        weapon
    }

    #[test]
    fn each_effect_reaches_only_what_its_own_targets_match() {
        let (_, hits) = used(sniper(), &["infantry"]);
        assert_eq!(hits, [(Target::Unit(1), 10.0), (Target::Unit(1), 250.0)]);
        let (_, hits) = used(sniper(), &["tank"]);
        assert_eq!(hits, [(Target::Unit(1), 10.0)]);
    }

    #[test]
    fn a_things_last_hit_is_the_last_effect_to_reach_it() {
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![sniper()]), soldier(vec![])],
            at_war(),
        );
        w.rules.damage_filters.insert("gunfire".into(), 4);
        let mut hits = Vec::new();
        let (s, t) = (Target::Unit(0), Target::Unit(1));
        w.strike(&sniper(), w.position(t), t, (s, 1), false, &mut hits);
        for hit in hits {
            w.land(hit);
        }
        let struck = w.units[1].struck.unwrap();
        assert_eq!((struck.filter, struck.damage), (4, 250.0));
    }

    #[test]
    fn effects_after_the_one_that_kills_still_land_before_the_death_counts() {
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![sniper()]), soldier(vec![])],
            at_war(),
        );
        w.rules.damage_filters.insert("gunfire".into(), 4);
        // Its death is picked by the gunfire, not the pierce that killed.
        let deaths = vec![
            tgmap::Death {
                ids: "shot".into(),
                filter: 4,
                threshold: 100.0,
                weight: 1.0,
                length: 1.0,
            },
            tgmap::Death {
                ids: "pierced".into(),
                filter: tgmap::ANY_DAMAGE,
                threshold: 0.0,
                weight: 1.0,
                length: 1.0,
            },
        ];
        w.rules.deaths.insert("soldier".into(), deaths);
        w.units[1].health = 5.0;
        let mut hits = Vec::new();
        let (s, t) = (Target::Unit(0), Target::Unit(1));
        w.strike(&sniper(), w.position(t), t, (s, 1), false, &mut hits);
        w.land_all(hits, &mut Vec::new());
        assert!(!w.units[1].alive());
        assert_eq!(w.units[1].death, Some(Id::from("shot")));
    }

    #[test]
    fn what_goes_to_the_user_lands_first_and_isnt_an_enemys() {
        let mut charge = rifle(1.0, 200.0);
        charge
            .hits
            .push(hit("self", true, Does::Add(of("explosive", 900.0))));
        let (mut w, hits) = used(charge.clone(), &["infantry"]);
        assert_eq!(hits, [(Target::Unit(0), 900.0), (Target::Unit(1), 200.0)]);
        // Resistance lessens it.
        let resists = &mut w.units[0].fighter.as_mut().unwrap().resists;
        resists.insert("explosive".into(), 0.5);
        let mut hits = Vec::new();
        w.use_on_user(&charge, (Target::Unit(0), 1), &mut hits);
        assert_eq!(hits[0].amount, 450.0);
        let mut hits = Vec::new();
        w.use_on_user(&charge, (Target::Unit(0), 1), &mut hits);
        w.use_on_user(&charge, (Target::Unit(0), 1), &mut hits);
        for hit in hits {
            w.land(hit);
        }
        // It's dead, credited to no enemy, and never struck by one.
        assert!(!w.units[0].alive());
        assert!(w.units[0].struck.is_none() && w.units[0].besieged.is_none());
    }

    #[test]
    fn main_damage_only_for_the_user_spares_what_its_aimed_at() {
        let mut pole = rifle(5.0, 0.0);
        pole.hits = vec![hit("self", true, Does::Main(of("blast", 25.0)))];
        let (_, hits) = used(pole, &["infantry"]);
        assert_eq!(hits, [(Target::Unit(0), 25.0)]);
    }

    #[test]
    fn an_area_centred_on_its_user_hits_round_it_wherever_it_was_aimed() {
        let mut charge = rifle(12.0, 50.0);
        charge.area = 1.0;
        charge.centred_on_user = true;
        let (_, hits) = used(charge.clone(), &["infantry"]);
        assert!(hits.is_empty());
        let mut w = battle(
            &[1, 2, 2],
            vec![soldier(vec![]), soldier(vec![]), soldier(vec![])],
            at_war(),
        );
        w.units[2].position = [10.5, 10.0];
        let (s, t) = (Target::Unit(0), Target::Unit(1));
        let mut hits = Vec::new();
        let at = w.impact(s, &charge, w.position(t));
        w.strike(&charge, at, t, (s, 1), false, &mut hits);
        let hit: Vec<Target> = hits.iter().map(|h| h.target).collect();
        assert_eq!(hit, [Target::Unit(2)]);
    }

    #[test]
    fn falloff_lessens_main_damage_and_morale_but_not_additive_damage() {
        let mut shell = rifle(12.0, 100.0);
        shell.targets = Default::default();
        (shell.area, shell.full_within, shell.edge_damage) = (4.0, 0.0, 0.5);
        shell
            .hits
            .push(hit("", false, Does::Add(of("explosive", 40.0))));
        shell.hits.push(hit("", false, Does::Morale(2.0)));
        let w = battle(&[1, 2], vec![soldier(vec![]), soldier(vec![])], at_war());
        let (s, t) = (Target::Unit(0), Target::Unit(1));
        let mut hits = Vec::new();
        let at = [w.position(t)[0] + 4.0, w.position(t)[1]];
        w.strike(&shell, at, t, (s, 1), false, &mut hits);
        let done: Vec<(f32, f32)> = hits.iter().map(|h| (h.amount, h.morale)).collect();
        assert_eq!(done, [(50.0, 0.0), (40.0, 0.0), (0.0, 1.0)]);
    }
}
