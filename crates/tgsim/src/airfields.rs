//! Airfields' aircraft: the stock each keeps and rebuilds, and the air
//! attacks it sends out, with their escorts, from its runway to the
//! target and home again.

use std::f32::consts::{FRAC_PI_4, PI};

use tgmap::{AirJob, Id, Weapon};

use crate::delivery::Plunge;
use crate::targets::User;
use crate::{Left, Notice, Piece, Projectile, Refused, State, TICK, Target, World, distance};

/// How far short of its target a plane starts its run in, as a share of
/// its longest weapon's range.
const STANDOFF: f32 = 1.75;

/// The least the cosine of the angle between where a plane points and its
/// target comes to for it to drop on it.
const POINTED: f32 = 0.9;

/// How near the heading it's turning to a plane has to come to have turned.
const TURNED: f32 = 0.05;

/// How long, in seconds, a plane keeps making for the point its run in
/// starts from before starting it anyway, as one that can't turn tightly
/// enough to reach it would never get there.
const APPROACH_SECONDS: f32 = 30.0;

/// How near an escort comes to its place beside the plane it flies with
/// to hold it there.
const HOLD: f32 = 3.0;

/// A place in an airfield's stock, for one plane.
#[derive(Debug, Clone, PartialEq)]
pub struct Berth {
    /// The plane's template.
    pub template: Id,
    /// Its health as a share of its most: 0 for a place that's empty.
    pub health: f32,
    /// Whether its plane is away, by index into [`World::sorties`].
    pub out: Option<usize>,
}

impl Berth {
    /// Whether its plane is at home, at full health.
    pub fn ready(&self) -> bool {
        self.out.is_none() && self.health >= 1.0
    }
}

/// A plane an airfield has sent out.
#[derive(Debug, Clone)]
pub struct Sortie {
    /// Its airfield, by index into [`World::buildings`], and its place in
    /// the airfield's stock.
    pub airfield: usize,
    pub berth: usize,
    pub template: Id,
    /// The side it flies for, which keeps it if its airfield changes hands.
    pub owner: u32,
    /// The planes sent out on the same order share a wave.
    pub wave: u32,
    pub job: Job,
    pub stage: Stage,
    /// The plane as a unit, by index into [`World::units`], once it's off.
    pub plane: Option<usize>,
    /// How far above the ground it flies.
    pub height: f32,
    /// The tick it takes off on.
    takes_off: u64,
    /// Its health as a share of its most when it takes off.
    health: f32,
    /// What it has left to pay its weapons' mana with.
    pub mana: f32,
    /// The thing it's after now, for an attack on a thing.
    pub target: Option<Target>,
    leg: Leg,
    /// Ticks it's flown the leg it's on.
    leg_ticks: u32,
    /// The side the next turn away goes to: 1 for the left.
    side: f32,
    /// Where it flies from the attack's lead plane, for an escort.
    slot: [f32; 2],
    /// The speed it's flying at.
    speed: f32,
    plunge: Option<Plunge>,
}

/// What a plane is sent out for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Job {
    /// An air attack.
    Attack(AirAim),
    /// Flying with its wave's attack, going after what last hit any of
    /// them.
    Escort { after: Option<Target> },
}

/// Where an air attack is aimed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AirAim {
    Thing(Piece),
    Place([f32; 2]),
}

/// Where a plane is in its sortie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// At its airfield, waiting its turn on the runway.
    Waiting,
    /// Off on its job.
    Out,
    /// Flying home.
    Home,
    /// Back in its stock, since this tick.
    Landed(u64),
    /// Shot down, falling.
    Falling,
    /// Down, since this tick.
    Down(u64),
    /// Gone with the airfield it can't come home to, since this tick.
    Lost(u64),
}

impl Stage {
    /// Whether it's over, the plane home or gone.
    pub fn over(self) -> bool {
        matches!(self, Stage::Landed(_) | Stage::Down(_) | Stage::Lost(_))
    }
}

/// A leg of a pass over the target.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Leg {
    /// To the point short of the target the run in starts from.
    Approach([f32; 2]),
    /// In at the target, dropping down.
    Run,
    /// Straight ahead, back up to its height.
    Climb,
    /// Round to come at the target again.
    Turn,
}

impl World {
    /// What the building at `b` keeps and sends out, if it has a stock of
    /// aircraft.
    pub fn airfield(&self, b: usize) -> Option<&tgmap::Airfield> {
        self.rules.airfields.get(&self.buildings.get(b)?.template)
    }

    /// The stock a building of `template` has, every place full if
    /// `stocked`, else empty.
    pub(crate) fn hangar(&self, template: &Id, stocked: bool) -> Vec<crate::Berth> {
        let stock = self.rules.airfields.get(template).map(|a| &a.stock);
        (stock.into_iter().flatten())
            .map(|t| Berth {
                template: t.clone(),
                health: if stocked { 1.0 } else { 0.0 },
                out: None,
            })
            .collect()
    }

    /// Empties the stock of the building at `b`, as one put up in play
    /// starts.
    pub(crate) fn empty_hangar(&mut self, b: usize) {
        for berth in &mut self.buildings[b].hangar {
            berth.health = 0.0;
        }
    }

    /// The places in the building at `b`'s stock whose planes are ready
    /// for the group it sends on `job`, at most as many as go out.
    fn ready_for(&self, b: usize, job: AirJob) -> Vec<usize> {
        let Some(group) = self.airfield(b).and_then(|a| a.group(job)) else {
            return Vec::new();
        };
        let hangar = &self.buildings[b].hangar;
        let ready = (hangar.iter().enumerate())
            .filter(|(_, berth)| berth.ready() && group.templates.contains(&berth.template))
            .map(|(i, _)| i);
        let ready: Vec<usize> = ready.take(group.most).collect();
        match ready.len() >= group.fewest.max(1) {
            true => ready,
            false => Vec::new(),
        }
    }

    /// Whether the building at `b` has enough planes ready to send out on
    /// `job`.
    pub fn planes_ready(&self, b: usize, job: AirJob) -> bool {
        !self.ready_for(b, job).is_empty()
    }

    /// What an air attack from the building at `b` costs in money.
    pub fn air_attack_cost(&self, b: usize) -> f32 {
        let money = crate::economy::money(&self.rules.economy);
        (self.airfield(b))
            .and_then(|a| a.attack_cost.get(&Id::from(money)))
            .copied()
            .unwrap_or(0.0)
    }

    /// The building at `b`, if it's `player`'s and can send out an air
    /// attack: a standing airfield with planes for one, neither going up
    /// nor packing or unpacking.
    fn attacking_airfield(&self, player: u32, b: usize) -> Result<(), Refused> {
        let building = self.standing_for(player, b)?;
        let offensive = self.airfield(b).and_then(|a| a.group(AirJob::Offensive));
        if building.busy() || offensive.is_none() {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    /// Whether `player` may order an air attack from the building at `b`
    /// now, on `on`: one its side can't afford is taken, to tell it so.
    pub(crate) fn may_air_attack(
        &self,
        player: u32,
        b: usize,
        on: Option<Piece>,
    ) -> Result<(), Refused> {
        self.attacking_airfield(player, b)?;
        if on.is_some_and(|p| !self.piece_exists(p)) {
            return Err(Refused::Unable);
        }
        if !self.affords_air_attack(player, b) {
            return Ok(());
        }
        if !self.planes_ready(b, AirJob::Offensive) {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    fn affords_air_attack(&self, player: u32, b: usize) -> bool {
        self.air_attack_cost(b) <= self.stockpile(player)
    }

    fn piece_exists(&self, piece: Piece) -> bool {
        match piece {
            Piece::Organization(r) => r < self.organizations.len(),
            Piece::Building(b) => b < self.buildings.len(),
        }
    }

    /// Sends an air attack from the building at `b` on `on`, or at `at`
    /// for none, with its escort if that's on, paying for it; or tells
    /// its side it can't afford one.
    pub(crate) fn order_air_attack(
        &mut self,
        player: u32,
        b: usize,
        at: [f32; 2],
        on: Option<Piece>,
    ) -> Result<(), Refused> {
        self.may_air_attack(player, b, on)?;
        if !self.affords_air_attack(player, b) {
            let cost = self.air_attack_cost(b);
            (self.notices).push(Notice::AirAttackUnaffordable {
                player,
                building: b,
                cost,
            });
            return Ok(());
        }
        let cost = self.air_attack_cost(b);
        let left = self.stockpile(player) - cost;
        self.set_stockpile(player, left);
        let aim = match on {
            Some(piece) => AirAim::Thing(piece),
            None => AirAim::Place(self.pathing.inside(at)),
        };
        let wave = self.next_wave;
        self.next_wave += 1;
        let attack = self.ready_for(b, AirJob::Offensive);
        let escort = match self.buildings[b].escort {
            true => self.ready_for(b, AirJob::Escort),
            false => Vec::new(),
        };
        let slots = (self.airfield(b))
            .and_then(|a| a.group(AirJob::Escort))
            .map(|g| g.formation.clone())
            .unwrap_or_default();
        let planes = (attack
            .into_iter()
            .map(|berth| (berth, Job::Attack(aim), [0.0; 2])))
        .chain(escort.into_iter().enumerate().map(|(i, berth)| {
            let [x, y, _] = slots.get(i).copied().unwrap_or([0.0; 3]);
            (berth, Job::Escort { after: None }, [x, y])
        }));
        let planes: Vec<_> = planes.collect();
        for (i, (berth, job, slot)) in planes.into_iter().enumerate() {
            self.send_out(b, berth, wave, job, slot, i);
        }
        Ok(())
    }

    /// Sets the building at `b`'s escort on or off.
    pub(crate) fn set_escort(&mut self, player: u32, b: usize, on: bool) -> Result<(), Refused> {
        self.standing_for(player, b)?;
        if self
            .airfield(b)
            .and_then(|a| a.group(AirJob::Escort))
            .is_none()
        {
            return Err(Refused::Unable);
        }
        self.buildings[b].escort = on;
        if !on {
            // What's escorting goes home, and what's yet to take off stays.
            for s in 0..self.sorties.len() {
                let sortie = &mut self.sorties[s];
                if sortie.airfield != b || !matches!(sortie.job, Job::Escort { .. }) {
                    continue;
                }
                if sortie.stage == Stage::Waiting {
                    sortie.stage = Stage::Landed(self.ticks);
                    self.buildings[b].hangar[sortie.berth].out = None;
                }
                self.head_home(s);
            }
        }
        Ok(())
    }

    /// Whether `player` may set the building at `b`'s escort.
    pub(crate) fn may_set_escort(&self, player: u32, b: usize) -> Result<(), Refused> {
        self.standing_for(player, b)?;
        match self.airfield(b).and_then(|a| a.group(AirJob::Escort)) {
            Some(_) => Ok(()),
            None => Err(Refused::Unable),
        }
    }

    /// Readies the plane in the building at `b`'s place `berth` to take
    /// off as the `order`th of those sent out together.
    fn send_out(
        &mut self,
        b: usize,
        berth: usize,
        wave: u32,
        job: Job,
        slot: [f32; 2],
        order: usize,
    ) {
        let delay = self.airfield(b).map_or(0.0, |a| a.runway.delay);
        let wait = (order as f32 * delay / TICK).round() as u64;
        let building = &self.buildings[b];
        let place = &building.hangar[berth];
        let template = place.template.clone();
        let height = self.rules.aircraft.get(&template).map_or(0.0, |a| a.height);
        let s = self.sorties.len();
        self.sorties.push(Sortie {
            airfield: b,
            berth,
            template,
            owner: building.owner,
            wave,
            job,
            stage: Stage::Waiting,
            plane: None,
            height,
            takes_off: self.ticks + wait,
            health: place.health,
            mana: 0.0,
            target: None,
            leg: Leg::Turn,
            leg_ticks: 0,
            side: 1.0,
            slot,
            speed: 0.0,
            plunge: None,
        });
        self.buildings[b].hangar[berth].out = Some(s);
    }

    /// Where the building at `b`'s runway starts and the way it runs.
    fn runway(&self, b: usize) -> ([f32; 2], [f32; 2]) {
        let building = &self.buildings[b];
        let runway = self.airfield(b).map(|a| a.runway).unwrap_or_default();
        let at = tgmap::from_local(building.position, building.facing, runway.offset);
        let way = tgmap::from_local([0.0; 2], building.facing, runway.orientation);
        (at, crate::normalised(way).unwrap_or(building.facing))
    }

    /// Takes off, flies on and brings home or down each plane out.
    pub(crate) fn fly_sorties(&mut self) {
        for s in 0..self.sorties.len() {
            match self.sorties[s].stage {
                Stage::Waiting if self.ticks >= self.sorties[s].takes_off => self.take_off(s),
                Stage::Out | Stage::Home => {
                    let shot = self.sorties[s]
                        .plane
                        .is_some_and(|u| self.units[u].died.is_some());
                    match shot {
                        true => self.sortie_shot_down(s),
                        false => self.fly_sortie(s),
                    }
                }
                Stage::Falling => self.fall_sortie(s),
                _ => {}
            }
        }
    }

    /// Puts the plane on the runway and sends it off, unless its airfield
    /// can no longer send it.
    fn take_off(&mut self, s: usize) {
        let sortie = &self.sorties[s];
        let b = sortie.airfield;
        let (owner, template) = (sortie.owner, sortie.template.clone());
        let (at, way) = self.runway(b);
        let raised = (self.buildings[b].standing())
            .then(|| self.raise(&template, owner, at, way))
            .flatten();
        let Some(r) = raised else {
            log::debug!("{template} couldn't take off from building {b}");
            self.sorties[s].stage = Stage::Lost(self.ticks);
            self.buildings[b].hangar[self.sorties[s].berth].out = None;
            return;
        };
        let u = self.organizations[r].units[0];
        let health = self.max_health(u) * self.sorties[s].health;
        let unit = &mut self.units[u];
        unit.aloft = true;
        unit.health = health;
        let mana = unit.fighter.as_ref().map_or(0.0, |f| f.mana);
        let speed = self.flying_speed(u);
        let sortie = &mut self.sorties[s];
        (sortie.plane, sortie.stage, sortie.mana, sortie.speed) =
            (Some(u), Stage::Out, mana, speed);
        log::debug!("{template} {} took off from building {b}", self.units[u].id);
    }

    /// How fast the plane at `u` flies: its top speed as its properties
    /// change it.
    fn flying_speed(&self, u: usize) -> f32 {
        self.top_speed(u, 0.0)
    }

    /// The radius of the circle the plane at `u` turns in.
    fn turn_radius(&self, u: usize) -> f32 {
        let template = &self.units[u].template;
        self.rules
            .aircraft
            .get(template)
            .map_or(0.0, |a| a.turn_radius)
    }

    /// The radians the plane at `u` turns through in a tick.
    fn turning(&self, u: usize, speed: f32) -> f32 {
        let radius = self.turn_radius(u);
        match radius > 0.0 {
            true => speed / radius * TICK,
            false => PI,
        }
    }

    /// The plane at `u`'s weapons, and the range of its longest.
    fn plane_weapons(&self, u: usize) -> (Vec<(usize, &Weapon)>, f32) {
        let weapons = self.units[u]
            .fighter
            .as_ref()
            .map_or(&[][..], |f| &f.weapons);
        let had: Vec<_> = (weapons.iter().enumerate())
            .filter(|(_, w)| self.has(u, w))
            .collect();
        let longest = had.iter().map(|(_, w)| w.range).fold(0.0, f32::max);
        (had, longest)
    }

    /// Whether the plane of sortie `s` can still use any of its weapons.
    fn has_load(&self, s: usize, u: usize) -> bool {
        let mana = self.sorties[s].mana;
        let (weapons, _) = self.plane_weapons(u);
        weapons.iter().any(|(_, w)| w.mana_cost <= mana)
    }

    /// Whether the plane at `u` of sortie `s`'s side can attack `t`.
    fn can_attack(&self, s: usize, u: usize, t: Target) -> bool {
        if self.fighter(t).is_none() || t == Target::Unit(u) {
            return false;
        }
        let user = User {
            player: self.sorties[s].owner,
            thing: Some(Target::Unit(u)),
        };
        let (weapons, _) = self.plane_weapons(u);
        weapons
            .iter()
            .any(|(_, w)| self.reaches(user, &w.targets, t))
    }

    /// What sortie `s` attacks now, for an attack on a thing: the same as
    /// before while it can be attacked, else for a regiment the nearest of
    /// its things that can be; none once there's nothing left to attack.
    fn pick_target(&mut self, s: usize, u: usize) -> Option<Target> {
        let piece = match self.sorties[s].job {
            Job::Attack(AirAim::Thing(piece)) => piece,
            Job::Escort { after: Some(t) } => {
                return self.can_attack(s, u, t).then_some(t);
            }
            _ => return None,
        };
        if let Some(t) = self.sorties[s].target.filter(|&t| self.can_attack(s, u, t)) {
            return Some(t);
        }
        let from = self.units[u].position;
        let found = match piece {
            Piece::Building(b) => Some(Target::Building(b)).filter(|&t| self.can_attack(s, u, t)),
            Piece::Organization(r) => (self.survivors(r))
                .map(Target::Unit)
                .filter(|&t| self.can_attack(s, u, t))
                .min_by(|&a, &b| {
                    let (a, b) = (self.reach(from, a), self.reach(from, b));
                    a.total_cmp(&b)
                }),
        };
        self.sorties[s].target = found;
        found
    }

    /// Where sortie `s` aims, ahead of a moving target by as far as it
    /// moves in the time the plane takes to get there.
    fn aim_point(&self, s: usize, u: usize) -> Option<[f32; 2]> {
        let from = self.units[u].position;
        match (self.sorties[s].job, self.sorties[s].target) {
            (Job::Attack(AirAim::Place(at)), _) => Some(at),
            (_, Some(t)) => {
                let at = self.position(t);
                let velocity = match t {
                    Target::Unit(v) => {
                        let v = &self.units[v];
                        [0, 1].map(|i| (v.position[i] - v.previous[i]) / TICK)
                    }
                    Target::Building(_) => [0.0; 2],
                };
                let speed = self.sorties[s].speed.max(f32::MIN_POSITIVE);
                let seconds = distance(from, at) / speed;
                Some([0, 1].map(|i| at[i] + velocity[i] * seconds))
            }
            _ => None,
        }
    }

    /// Flies the plane of sortie `s` a tick on.
    fn fly_sortie(&mut self, s: usize) {
        let Some(u) = self.sorties[s].plane else {
            return;
        };
        self.sorties[s].speed = self.flying_speed(u);
        match self.sorties[s].stage {
            Stage::Home => self.fly_home(s, u),
            _ => match self.sorties[s].job {
                Job::Attack(_) => self.fly_attack(s, u),
                Job::Escort { .. } => self.fly_escort(s, u),
            },
        }
    }

    /// A tick of an attack: passes over the target until it's gone or the
    /// plane's load is spent, then home.
    fn fly_attack(&mut self, s: usize, u: usize) {
        let target = self.pick_target(s, u);
        let on_thing = !matches!(self.sorties[s].job, Job::Attack(AirAim::Place(_)));
        if on_thing && target.is_none() || !self.has_load(s, u) {
            self.head_home(s);
            return self.fly_home(s, u);
        }
        let Some(aim) = self.aim_point(s, u) else {
            self.head_home(s);
            return self.fly_home(s, u);
        };
        self.make_pass(s, u, aim);
    }

    /// A tick of a pass over `aim`, at the plane's height, its run in
    /// dropping to half that.
    fn make_pass(&mut self, s: usize, u: usize, aim: [f32; 2]) {
        let (_, longest) = self.plane_weapons(u);
        let standoff = STANDOFF * longest;
        let full = self
            .rules
            .aircraft
            .get(&self.sorties[s].template)
            .map_or(0.0, |a| a.height);
        let half = full / 2.0;
        let speed = self.sorties[s].speed;
        // It's down to half its height by the time it would reach the
        // target from where its run starts.
        let dive = match standoff > 0.0 {
            true => half * speed / standoff,
            false => half,
        };
        let unit = &self.units[u];
        let (at, facing) = (unit.position, unit.facing);
        let towards = crate::normalised([aim[0] - at[0], aim[1] - at[1]]).unwrap_or(facing);
        let ahead = facing[0] * towards[0] + facing[1] * towards[1];
        let sortie = &mut self.sorties[s];
        sortie.leg_ticks += 1;
        let leg = sortie.leg;
        let next = match leg {
            Leg::Approach(point) => {
                let there = distance(at, point) <= speed;
                let long = sortie.leg_ticks as f32 * TICK > APPROACH_SECONDS;
                let close = distance(at, aim) <= standoff && ahead > 0.0;
                (there || close || long).then_some(Leg::Run)
            }
            Leg::Run => {
                sortie.height = (sortie.height - dive * TICK).max(half);
                (sortie.height <= half || ahead <= 0.0).then_some(Leg::Climb)
            }
            Leg::Climb => {
                sortie.height = (sortie.height + dive * TICK).min(full);
                (sortie.height >= full).then_some(Leg::Turn)
            }
            Leg::Turn => {
                let want = rotated(towards, sortie.side * FRAC_PI_4);
                (angle(facing, want).abs() <= TURNED).then(|| {
                    sortie.side = -sortie.side;
                    Leg::Approach([
                        aim[0] - towards[0] * standoff,
                        aim[1] - towards[1] * standoff,
                    ])
                })
            }
        };
        if let Some(next) = next {
            (sortie.leg, sortie.leg_ticks) = (next, 0);
        }
        let want = match sortie.leg {
            Leg::Approach(point) => crate::normalised([point[0] - at[0], point[1] - at[1]]),
            Leg::Run => Some(towards),
            Leg::Climb => Some(facing),
            Leg::Turn => Some(rotated(towards, sortie.side * FRAC_PI_4)),
        };
        let most = self.turning(u, speed);
        self.steer(u, want.unwrap_or(facing), most, speed);
    }

    /// A tick of an escort: with its wave's attack, or after what last hit
    /// one of them, until the attack's done.
    fn fly_escort(&mut self, s: usize, u: usize) {
        let wave = self.sorties[s].wave;
        let attacking =
            |o: &Sortie| o.wave == wave && matches!(o.job, Job::Attack(_)) && !o.stage.over();
        let attacks: Vec<usize> = (0..self.sorties.len())
            .filter(|&o| attacking(&self.sorties[o]))
            .collect();
        let still_out = attacks
            .iter()
            .any(|&o| matches!(self.sorties[o].stage, Stage::Out | Stage::Waiting));
        if !still_out || !self.has_load(s, u) {
            self.head_home(s);
            return self.fly_home(s, u);
        }
        // What most lately hit any of the wave's planes.
        let wave_planes = (0..self.sorties.len())
            .filter(|&o| self.sorties[o].wave == wave && !self.sorties[o].stage.over())
            .filter_map(|o| self.sorties[o].plane);
        let owner = self.sorties[s].owner;
        let hit = (wave_planes.filter_map(|p| self.units[p].struck))
            .filter(|h| h.by != owner)
            .max_by_key(|h| h.at)
            .and_then(|h| h.from);
        if let Some(t) = hit.filter(|&t| self.can_attack(s, u, t)) {
            self.sorties[s].job = Job::Escort { after: Some(t) };
        }
        if let Some(t) = self.pick_target(s, u) {
            self.sorties[s].target = Some(t);
            if let Some(aim) = self.aim_point(s, u) {
                return self.make_pass(s, u, aim);
            }
        }
        self.sorties[s].job = Job::Escort { after: None };
        self.sorties[s].target = None;
        // In its place beside the attack's first plane still out.
        let lead = (attacks.iter())
            .filter(|&&o| self.sorties[o].stage == Stage::Out)
            .find_map(|&o| self.sorties[o].plane);
        let Some(lead) = lead else {
            // Waiting on the attack to take off.
            let facing = self.units[u].facing;
            let speed = self.sorties[s].speed;
            return self.steer(u, facing, 0.0, speed);
        };
        let (lead_at, lead_facing) = (self.units[lead].position, self.units[lead].facing);
        let slot = tgmap::from_local(lead_at, lead_facing, self.sorties[s].slot);
        let at = self.units[u].position;
        let speed = self.sorties[s].speed;
        let lead_speed = self
            .sorties
            .iter()
            .find(|o| o.plane == Some(lead))
            .map_or(speed, |o| o.speed);
        if distance(at, slot) <= HOLD {
            let step = distance(at, slot).min(speed * TICK);
            let way = crate::normalised([slot[0] - at[0], slot[1] - at[1]]).unwrap_or(lead_facing);
            let held = [
                at[0] + way[0] * step + lead_facing[0] * lead_speed * TICK,
                at[1] + way[1] * step + lead_facing[1] * lead_speed * TICK,
            ];
            let held = self.pathing.inside(held);
            let unit = &mut self.units[u];
            (unit.previous, unit.position, unit.facing) = (at, held, lead_facing);
            unit.speed = lead_speed;
            return;
        }
        let want = crate::normalised([slot[0] - at[0], slot[1] - at[1]]).unwrap_or(lead_facing);
        let most = self.turning(u, speed);
        self.steer(u, want, most, speed);
    }

    /// Turns sortie `s` for home, where its airfield still stands; with
    /// its airfield destroyed or sold it dies where it is.
    fn head_home(&mut self, s: usize) {
        let sortie = &mut self.sorties[s];
        if sortie.stage != Stage::Out {
            return;
        }
        sortie.stage = Stage::Home;
        sortie.target = None;
        let b = sortie.airfield;
        let gone = matches!(
            self.buildings[b].state,
            State::Destroyed(_) | State::Sold(_)
        );
        if let (true, Some(u)) = (gone, sortie.plane) {
            log::debug!(
                "{} {} has no airfield to go home to",
                sortie.template,
                self.units[u].id
            );
            let ticks = self.ticks;
            self.units[u].die(ticks);
            self.mourn(u);
            self.lost_member(u);
        }
    }

    /// Where sortie `s` goes home to: its airfield's runway, or the truck
    /// it packed into.
    fn home(&self, s: usize) -> [f32; 2] {
        let b = self.sorties[s].airfield;
        match self.buildings[b].state {
            State::Packed(_, r) => self.centre(r),
            _ => self.runway(b).0,
        }
    }

    /// A tick of the flight home, at its height, landing once there.
    fn fly_home(&mut self, s: usize, u: usize) {
        let home = self.home(s);
        let full = self
            .rules
            .aircraft
            .get(&self.sorties[s].template)
            .map_or(0.0, |a| a.height);
        let sortie = &mut self.sorties[s];
        let speed = sortie.speed;
        sortie.height = (sortie.height + speed * TICK).min(full);
        let at = self.units[u].position;
        if distance(at, home) <= speed * TICK {
            return self.land_sortie(s, u);
        }
        let facing = self.units[u].facing;
        let want = crate::normalised([home[0] - at[0], home[1] - at[1]]).unwrap_or(facing);
        // Near home it makes straight for it, rather than circling it.
        let radius = self.turn_radius(u);
        let most = match distance(at, home) <= 2.0 * radius {
            true => PI,
            false => self.turning(u, speed),
        };
        self.steer(u, want, most, speed);
    }

    /// Brings the plane of sortie `s` back into its place in its
    /// airfield's stock, keeping its health; one whose airfield packed is
    /// taken off the map.
    fn land_sortie(&mut self, s: usize, u: usize) {
        let ticks = self.ticks;
        let health = self.units[u].health / self.max_health(u).max(f32::MIN_POSITIVE);
        let unit = &mut self.units[u];
        (unit.aloft, unit.left) = (false, Some(Left::Disbanded(ticks)));
        let sortie = &mut self.sorties[s];
        let (b, berth) = (sortie.airfield, sortie.berth);
        let packed = matches!(self.buildings[b].state, State::Packed(..));
        sortie.stage = match packed {
            true => Stage::Lost(ticks),
            false => Stage::Landed(ticks),
        };
        let place = &mut self.buildings[b].hangar[berth];
        place.out = None;
        if !packed {
            place.health = health.clamp(0.0, 1.0);
        }
        log::debug!(
            "{} {} landed at building {b}",
            self.units[u].template,
            self.units[u].id
        );
    }

    /// Has the plane of sortie `s`, just shot down, drop out of its flight,
    /// freeing its place in its airfield's stock.
    fn sortie_shot_down(&mut self, s: usize) {
        let sortie = &mut self.sorties[s];
        sortie.stage = Stage::Falling;
        sortie.plunge = Some(Plunge::new(sortie.speed));
        let (b, berth) = (sortie.airfield, sortie.berth);
        let place = &mut self.buildings[b].hangar[berth];
        (place.out, place.health) = (None, 0.0);
        self.fall_sortie(s);
    }

    /// Brings the plane of sortie `s` down a tick further, on the way it
    /// was heading, until it comes to rest at its taxi height over the
    /// ground, where its on-death ability goes off.
    fn fall_sortie(&mut self, s: usize) {
        let (Some(u), Some(mut plunge)) = (self.sorties[s].plane, self.sorties[s].plunge) else {
            return;
        };
        let pull = self.rules.plane_fall_tweak * self.rules.gravity * TICK;
        let (across, sinking) = plunge.step(pull);
        self.sorties[s].plunge = Some(plunge);
        let unit = &self.units[u];
        let [fx, fy] = unit.facing;
        let on = [
            unit.position[0] + fx * across * TICK,
            unit.position[1] + fy * across * TICK,
        ];
        let at = self.pathing.inside(on);
        let unit = &mut self.units[u];
        (unit.previous, unit.position) = (unit.position, at);
        let taxi = self
            .rules
            .aircraft
            .get(&self.sorties[s].template)
            .map_or(0.0, |a| a.taxi_height);
        let sortie = &mut self.sorties[s];
        sortie.height -= sinking * TICK;
        if sortie.height > taxi {
            return;
        }
        sortie.height = taxi;
        sortie.stage = Stage::Down(self.ticks);
        let ticks = self.ticks;
        let unit = &mut self.units[u];
        (unit.aloft, unit.landed) = (false, Some(ticks));
        self.set_off_death(u);
    }

    /// Turns the plane at `u` towards `want` by at most `most` radians and
    /// flies it on at `speed`.
    fn steer(&mut self, u: usize, want: [f32; 2], most: f32, speed: f32) {
        let unit = &mut self.units[u];
        let turn = angle(unit.facing, want).clamp(-most, most);
        unit.facing = rotated(unit.facing, turn);
        unit.previous = unit.position;
        let [fx, fy] = unit.facing;
        let on = [
            unit.position[0] + fx * speed * TICK,
            unit.position[1] + fy * speed * TICK,
        ];
        unit.position = self.pathing.inside(on);
        unit.speed = speed;
    }

    /// Has each plane out on an attack drop on, or fire at, its target as
    /// it passes: on its run in, pointed at it and within reach.
    pub(crate) fn air_shots(
        &mut self,
        hits: &mut Vec<crate::combat::Hit>,
        launched: &mut Vec<Projectile>,
    ) {
        for s in 0..self.sorties.len() {
            let sortie = &self.sorties[s];
            let Some(u) = sortie.plane.filter(|_| sortie.stage == Stage::Out) else {
                continue;
            };
            if !self.units[u].alive() {
                continue;
            }
            for ago in &mut self.units[u].fired_ago {
                *ago += TICK;
            }
            if self.sorties[s].leg != Leg::Run {
                continue;
            }
            let sortie = &self.sorties[s];
            let (place, target) = match (sortie.job, sortie.target) {
                (Job::Attack(AirAim::Place(at)), _) => (Some(at), None),
                (_, Some(t)) => (None, Some(t)),
                _ => continue,
            };
            let unit = &self.units[u];
            let (at, facing) = (unit.position, unit.facing);
            let towards = place.unwrap_or_else(|| target.map_or(at, |t| self.position(t)));
            let Some(way) = crate::normalised([towards[0] - at[0], towards[1] - at[1]]) else {
                continue;
            };
            if facing[0] * way[0] + facing[1] * way[1] < POINTED {
                continue;
            }
            let rate = self.fire_rate(Target::Unit(u));
            let user = User {
                player: sortie.owner,
                thing: Some(Target::Unit(u)),
            };
            let mana = sortie.mana;
            let (weapons, _) = self.plane_weapons(u);
            let fired_ago = &unit.fired_ago;
            let usable = (weapons.into_iter()).filter(|(w, weapon)| {
                let ready =
                    fired_ago[*w].is_infinite() || fired_ago[*w] * rate >= weapon.recharge - 1e-4;
                let reach = match target {
                    Some(t) => {
                        self.reaches(user, &weapon.targets, t) && self.reach(at, t) <= weapon.range
                    }
                    // Only what hits an area can be aimed at the ground.
                    None => weapon.area > 0.0 && distance(at, towards) <= weapon.range,
                };
                ready && weapon.mana_cost <= mana && reach
            });
            let best = usable.fold(
                None,
                |best: Option<(usize, &Weapon)>, (w, weapon)| match best {
                    Some((_, b)) if b.choice.priority >= weapon.choice.priority => best,
                    _ => Some((w, weapon)),
                },
            );
            let Some((w, weapon)) = best.map(|(w, weapon)| (w, weapon.clone())) else {
                continue;
            };
            self.sorties[s].mana -= weapon.mana_cost;
            log::debug!(
                "{} {} used {w} on {target:?} at {towards:?}, {} mana left",
                self.units[u].template,
                self.units[u].id,
                self.sorties[s].mana
            );
            let t = target.unwrap_or(Target::Unit(u));
            self.use_weapon(u, w, &weapon, t, place, hits, launched);
        }
    }

    /// Rebuilds and repairs each airfield's planes: one place at a time,
    /// a damaged plane at home before an empty place, at its plane's
    /// resupply rate, slowed by its side's shortage and while under siege.
    pub(crate) fn rebuild_planes(&mut self) {
        for b in 0..self.buildings.len() {
            let building = &self.buildings[b];
            if building.hangar.is_empty() || !building.standing() || building.constructing {
                continue;
            }
            let home = |p: &&crate::Berth| p.out.is_none() && p.health < 1.0;
            let damaged = (building.hangar.iter().enumerate())
                .filter(|(_, p)| home(p))
                .find(|(_, p)| p.health > 0.0);
            let empty = (building.hangar.iter().enumerate()).find(|(_, p)| home(p));
            let Some((i, place)) = damaged.or(empty) else {
                continue;
            };
            let rate = self
                .rules
                .resupply_rates
                .get(&place.template)
                .copied()
                .unwrap_or(0.0);
            let short = self.short(building.owner).unwrap_or(0.0);
            let siege = match building.besieged.is_some() {
                true => self.rules.denizen_siege_rate,
                false => 1.0,
            };
            let gain = rate * (1.0 - short) * siege * TICK;
            let place = &mut self.buildings[b].hangar[i];
            place.health = (place.health + gain).min(1.0);
        }
    }

    /// How far above the ground the plane at `u` flies, for one an
    /// airfield sent out.
    pub fn flight_height(&self, u: usize) -> Option<f32> {
        (self.sorties.iter())
            .find(|s| s.plane == Some(u) && !s.stage.over())
            .map(|s| s.height)
    }
}

/// The angle from `from` to `to`, counter-clockwise positive.
fn angle(from: [f32; 2], to: [f32; 2]) -> f32 {
    let cross = from[0] * to[1] - from[1] * to[0];
    let dot = from[0] * to[0] + from[1] * to[1];
    libm::atan2f(cross, dot)
}

/// `v` turned `by` radians counter-clockwise.
fn rotated(v: [f32; 2], by: f32) -> [f32; 2] {
    let (sin, cos) = (libm::sinf(by), libm::cosf(by));
    [v[0] * cos - v[1] * sin, v[0] * sin + v[1] * cos]
}

crate::hash::digest!(Berth {
    template,
    health,
    out
});

crate::hash::digest!(Sortie {
    airfield,
    berth,
    template,
    owner,
    wave,
    job,
    stage,
    plane,
    height,
    takes_off,
    health,
    mana,
    target,
    leg,
    leg_ticks,
    side,
    slot,
    speed,
    plunge,
});

impl crate::hash::Digest for Job {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        match *self {
            Job::Attack(aim) => (0u8, aim).digest(h),
            Job::Escort { after } => (1u8, after).digest(h),
        }
    }
}

impl crate::hash::Digest for AirAim {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        match *self {
            AirAim::Thing(piece) => (0u8, piece).digest(h),
            AirAim::Place(at) => (1u8, at).digest(h),
        }
    }
}

impl crate::hash::Digest for Stage {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        match *self {
            Stage::Waiting => 0u8.digest(h),
            Stage::Out => 1u8.digest(h),
            Stage::Home => 2u8.digest(h),
            Stage::Landed(t) => (3u8, t).digest(h),
            Stage::Falling => 4u8.digest(h),
            Stage::Down(t) => (5u8, t).digest(h),
            Stage::Lost(t) => (6u8, t).digest(h),
        }
    }
}

impl crate::hash::Digest for Leg {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        match *self {
            Leg::Approach(at) => (0u8, at).digest(h),
            Leg::Run => 1u8.digest(h),
            Leg::Climb => 2u8.digest(h),
            Leg::Turn => 3u8.digest(h),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;
    use crate::{Command, Pathing};
    use tgmap::{AirGroup, Aircraft, Airfield, Domain, Runway};

    /// The airfield's index in [`World::buildings`].
    const FIELD: usize = 1;

    /// A plane of `template` that flies at `speed`, of `health`, armed
    /// with `weapons` and `mana` to pay for them.
    fn plane(
        template: &str,
        speed: f32,
        health: f32,
        weapons: Vec<Weapon>,
        mana: f32,
    ) -> tgmap::Organization {
        let mut o = truck();
        (o.template, o.unpacks) = (template.into(), None);
        let mut fighter = soldier(weapons);
        (fighter.health, fighter.mana) = (health, mana);
        fighter.flags = vec![Id::from("air")];
        let m = &mut o.members[0];
        (m.template, m.speed, m.fighter) = (template.into(), speed, Some(fighter));
        o
    }

    /// Bombs as the stock bomber's: 75 within 6, reaching 15, every 5
    /// seconds, at 5 of its 60 mana, on enemies on the ground.
    fn bomb() -> Weapon {
        let mut bomb = rifle(15.0, 75.0);
        (bomb.area, bomb.recharge, bomb.mana_cost) = (6.0, 5.0, 5.0);
        bomb.targets.excluded = vec![Id::from("air")];
        bomb
    }

    /// Player 1's airfield at 20,50 on a map 120 across, with 500 money:
    /// two bombers and two fighters, the attack one bomber, the escort
    /// one or two fighters, a runway from 5 ahead of it with a second
    /// between take-offs, and an attack costing 150. Player 2 has a house
    /// at 80,50.
    fn field() -> World {
        let mut w = economy();
        w.pathing = Pathing::uniform(120, Domain::Land);
        let mut airfield = building(6, 1, [20.0, 50.0]);
        airfield.template = "airfield".into();
        let house = building(7, 2, [80.0, 50.0]);
        w.add_buildings(&[airfield, house]);
        let ids = |ids: &[&str]| ids.iter().map(|&i| Id::from(i)).collect::<Vec<_>>();
        let group = |template: &str, [fewest, most]: [usize; 2], job| AirGroup {
            templates: ids(&[template]),
            fewest,
            most,
            jobs: vec![job],
            formation: vec![[-9.0, 15.0, -2.0], [9.0, 15.0, -2.0]],
        };
        w.rules.airfields.insert(
            Id::from("airfield"),
            Airfield {
                stock: ids(&["bomber", "bomber", "fighter", "fighter"]),
                groups: vec![
                    group("bomber", [1, 1], AirJob::Offensive),
                    group("fighter", [1, 2], AirJob::Escort),
                ],
                runway: Runway {
                    offset: [0.0, 5.0],
                    orientation: [-1.0, 0.0],
                    delay: 1.0,
                },
                attack_cost: [(Id::from("money"), 150.0)].into(),
            },
        );
        // The stock as the map has it.
        w.buildings[FIELD].hangar = w.hangar(&Id::from("airfield"), true);
        let mut gun = rifle(14.0, 10.0);
        gun.recharge = 0.2;
        for (template, speed, health, weapons, mana, height, radius) in [
            ("bomber", 7.0, 300.0, vec![bomb()], 60.0, 17.0, 18.0),
            ("fighter", 11.0, 100.0, vec![gun], 20.0, 15.0, 8.0),
        ] {
            let o = plane(template, speed, health, weapons, mana);
            w.rules.raisable.insert(Id::from(template), o);
            let aircraft = Aircraft {
                height,
                taxi_height: 1.0,
                turn_radius: radius,
            };
            w.rules.aircraft.insert(Id::from(template), aircraft);
            w.rules.resupply_rates.insert(Id::from(template), 0.05);
        }
        w
    }

    fn attack(on: Option<Piece>, at: [f32; 2]) -> Command {
        Command::AirAttack {
            building: FIELD,
            at,
            on,
        }
    }

    fn on_house() -> Command {
        attack(Some(Piece::Building(2)), [80.0, 50.0])
    }

    /// Plays until `done`, failing after `seconds`.
    fn until(w: &mut World, seconds: f32, done: impl Fn(&World) -> bool) {
        let started = w.ticks;
        while !done(w) {
            assert!(
                ((w.ticks - started) as f32) * TICK < seconds,
                "not done in {seconds}s"
            );
            w.tick();
        }
    }

    #[test]
    fn an_air_attack_takes_off_bombs_its_target_and_comes_home() {
        let mut w = field();
        assert!(w.buildings[FIELD].hangar.iter().all(Berth::ready));
        assert!(w.planes_ready(FIELD, AirJob::Offensive));
        w.apply(1, on_house()).unwrap();
        // Paid for as it goes out: one bomber, the first.
        assert_eq!(w.stockpile(1), 350.0);
        assert_eq!(w.sorties.len(), 1);
        assert_eq!(w.buildings[FIELD].hangar[0].out, Some(0));
        w.tick();
        let u = w.sorties[0].plane.expect("off");
        // From the runway, 5 ahead of the airfield, which faces -y,
        // along it.
        assert!(near(w.units[u].position, [20.0, 45.0]));
        assert!(near(w.units[u].facing, [1.0, 0.0]));
        assert!(w.units[u].in_air() && !w.commands(1, w.units[u].organization));
        assert_eq!(w.flight_height(u), Some(17.0));
        until(&mut w, 60.0, |w| w.buildings[2].health < 100.0);
        assert!(w.sorties[0].mana < 60.0);
        until(&mut w, 300.0, |w| w.sorties[0].stage.over());
        // The house went, and the bomber came home and back into its place.
        assert!(w.buildings[2].gone());
        assert!(matches!(w.sorties[0].stage, Stage::Landed(_)));
        assert!(!w.units[u].alive() && !w.units[u].in_air());
        assert!(w.buildings[FIELD].hangar[0].ready());
    }

    #[test]
    fn an_air_attack_the_side_cant_afford_tells_it_so_and_sends_nothing() {
        let mut w = field();
        w.set_stockpile(1, 100.0);
        // Taken, to be told, however few planes are ready.
        for berth in &mut w.buildings[FIELD].hangar[..2] {
            berth.health = 0.5;
        }
        assert_eq!(w.check(1, &on_house()), Ok(()));
        w.take_notices();
        assert_eq!(w.apply(1, on_house()), Ok(()));
        let told = Notice::AirAttackUnaffordable {
            player: 1,
            building: FIELD,
            cost: 150.0,
        };
        assert_eq!(w.take_notices(), [told]);
        assert!(w.sorties.is_empty() && w.stockpile(1) == 100.0);
    }

    #[test]
    fn without_planes_ready_nothing_goes_out_or_is_paid_or_said() {
        let mut w = field();
        // One being repaired, the other away.
        w.buildings[FIELD].hangar[0].health = 0.5;
        w.buildings[FIELD].hangar[1].out = Some(9);
        w.take_notices();
        assert_eq!(w.apply(1, on_house()), Err(Refused::Unable));
        assert!(w.take_notices().is_empty());
        assert!(w.sorties.is_empty() && w.stockpile(1) == 500.0);
        // Not for other sides, or buildings with no planes.
        let mut w = field();
        assert_eq!(w.apply(2, on_house()), Err(Refused::NotYours));
        let mut hq = on_house();
        if let Command::AirAttack { building, .. } = &mut hq {
            *building = 0;
        }
        assert_eq!(w.apply(1, hq), Err(Refused::Unable));
    }

    #[test]
    fn each_order_sends_another_group_while_planes_are_ready() {
        let mut w = field();
        w.apply(1, on_house()).unwrap();
        w.apply(1, on_house()).unwrap();
        assert_eq!(w.stockpile(1), 200.0);
        let berths: Vec<_> = w.sorties.iter().map(|s| s.berth).collect();
        assert_eq!(berths, [0, 1]);
        assert_ne!(w.sorties[0].wave, w.sorties[1].wave);
        assert_eq!(w.apply(1, on_house()), Err(Refused::Unable));
        assert_eq!(w.stockpile(1), 200.0);
    }

    #[test]
    fn a_place_is_bombed_until_the_bombs_run_out() {
        let mut w = field();
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        until(&mut w, 400.0, |w| w.sorties[0].stage == Stage::Home);
        // Twelve bombs of five from sixty.
        assert!(w.sorties[0].mana < 5.0, "{}", w.sorties[0].mana);
        until(&mut w, 200.0, |w| w.sorties[0].stage.over());
        assert!(w.buildings[FIELD].hangar[0].ready());
    }

    #[test]
    fn a_free_bomb_needing_what_the_side_lacks_doesnt_keep_it_out() {
        let mut w = field();
        let mut napalm = bomb();
        napalm.mana_cost = 0.0;
        napalm.choice.requires = vec![Id::from("tech_napalm")];
        let bomber = w.rules.raisable.get_mut(&Id::from("bomber")).unwrap();
        let fighter = bomber.members[0].fighter.as_mut().unwrap();
        fighter.weapons.push(napalm);
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        until(&mut w, 400.0, |w| w.sorties[0].stage == Stage::Home);
        assert!(w.sorties[0].mana < 5.0);
    }

    #[test]
    fn guns_that_hit_no_area_arent_fired_at_the_ground() {
        let mut w = field();
        let airfield = w.rules.airfields.get_mut(&Id::from("airfield")).unwrap();
        airfield.groups[0].templates = vec![Id::from("fighter")];
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        run(&mut w, 40.0);
        let u = w.sorties[0].plane.unwrap();
        assert_eq!(w.units[u].health, 100.0);
        assert!(w.units[u].shot.is_none());
    }

    #[test]
    fn on_a_regiment_it_bombs_its_nearest_thing_then_the_next() {
        let mut w = field();
        let mut o = truck();
        (o.template, o.unpacks, o.primary) = ("pair".into(), None, None);
        let mut second = o.members[0].clone();
        second.id = 1;
        o.members.push(second);
        o.formations[0].offsets = vec![[0.0, 0.0], [0.0, 10.0]];
        w.rules.raisable.insert(Id::from("pair"), o);
        let r = w
            .raise(&Id::from("pair"), 2, [80.0, 20.0], [0.0, 1.0])
            .unwrap();
        let units = w.organizations[r].units.clone();
        w.apply(1, attack(Some(Piece::Organization(r)), [80.0, 20.0]))
            .unwrap();
        until(&mut w, 30.0, |w| w.sorties[0].target.is_some());
        let first = w.sorties[0].target;
        let from = w.units[w.sorties[0].plane.unwrap()].position;
        let nearest = (units.iter().copied())
            .min_by(|&a, &b| {
                distance(from, w.units[a].position).total_cmp(&distance(from, w.units[b].position))
            })
            .unwrap();
        assert_eq!(first, Some(Target::Unit(nearest)));
        w.units[nearest].die(w.ticks);
        w.tick();
        let other = units.into_iter().find(|&v| v != nearest).unwrap();
        assert_eq!(w.sorties[0].target, Some(Target::Unit(other)));
        w.units[other].die(w.ticks);
        w.tick();
        assert_eq!(w.sorties[0].stage, Stage::Home);
    }

    #[test]
    fn a_plane_shot_down_falls_and_its_place_is_rebuilt() {
        let mut w = field();
        w.apply(1, on_house()).unwrap();
        run(&mut w, 3.0);
        let u = w.sorties[0].plane.unwrap();
        w.land(crate::combat::Hit::strike(Target::Unit(u), 1000.0, 2, None));
        let shot = w.ticks;
        w.tick();
        assert_eq!(w.sorties[0].stage, Stage::Falling);
        let place = &w.buildings[FIELD].hangar[0];
        assert!(place.out.is_none() && place.health < 0.01);
        until(&mut w, 10.0, |w| {
            matches!(w.sorties[0].stage, Stage::Down(_))
        });
        assert!(!w.units[u].in_air() && w.units[u].landed.is_some());
        // Rebuilt from the moment it's lost, twenty seconds from nothing
        // to whole.
        until(&mut w, 30.0, |w| w.buildings[FIELD].hangar[0].ready());
        let seconds = (w.ticks - shot) as f32 * TICK;
        assert!((seconds - 20.0).abs() < 0.5, "{seconds}s");
    }

    #[test]
    fn planes_are_rebuilt_one_place_at_a_time_the_damaged_first() {
        let mut w = field();
        w.empty_hangar(FIELD);
        w.buildings[FIELD].hangar[3].health = 0.5;
        run(&mut w, 1.0);
        let healths = |w: &World| {
            w.buildings[FIELD]
                .hangar
                .iter()
                .map(|p| p.health)
                .collect::<Vec<_>>()
        };
        let h = healths(&w);
        assert!(h[3] > 0.54 && h[..3].iter().all(|&h| h == 0.0), "{h:?}");
        run(&mut w, 10.0);
        let h = healths(&w);
        assert!(h[3] == 1.0 && h[0] > 0.0 && h[1] == 0.0, "{h:?}");
        // A quarter as fast under siege.
        let mut w = field();
        w.empty_hangar(FIELD);
        w.buildings[FIELD].besieged = Some(u64::MAX);
        run(&mut w, 4.0);
        let h = healths(&w);
        assert!((h[0] - 0.05).abs() < 0.01, "{h:?}");
    }

    #[test]
    fn an_airfield_put_up_in_play_starts_with_its_stock_empty() {
        let w = field();
        let hangar = w.hangar(&Id::from("airfield"), false);
        assert_eq!(hangar.len(), 4);
        assert!(hangar.iter().all(|p| p.health == 0.0 && p.out.is_none()));
    }

    #[test]
    fn a_plane_whose_airfield_is_destroyed_dies_turning_for_home() {
        let mut w = field();
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        run(&mut w, 2.0);
        let u = w.sorties[0].plane.unwrap();
        w.buildings[FIELD].state = State::Destroyed(w.ticks);
        // Still on its job, until that's done.
        run(&mut w, 2.0);
        assert!(w.units[u].alive());
        w.sorties[0].mana = 0.0;
        w.tick();
        assert!(!w.units[u].alive());
        until(&mut w, 10.0, |w| {
            matches!(w.sorties[0].stage, Stage::Down(_))
        });
    }

    #[test]
    fn an_escort_flies_with_the_attack_and_goes_after_what_hits_it() {
        let mut w = field();
        assert_eq!(
            w.apply(
                1,
                Command::Escort {
                    building: FIELD,
                    on: true
                }
            ),
            Ok(())
        );
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        // The bomber and both fighters, a second apart, for nothing more.
        let jobs: Vec<_> = w
            .sorties
            .iter()
            .map(|s| matches!(s.job, Job::Escort { .. }))
            .collect();
        assert_eq!(jobs, [false, true, true]);
        assert_eq!(w.stockpile(1), 350.0);
        run(&mut w, 2.5);
        assert!(w.sorties.iter().all(|s| s.stage == Stage::Out));
        run(&mut w, 8.0);
        let bomber = w.sorties[0].plane.unwrap();
        let escort = w.sorties[1].plane.unwrap();
        assert!(distance(w.units[bomber].position, w.units[escort].position) < 25.0);
        // An enemy hits the bomber.
        let enemy = w
            .raise(&Id::from("truck"), 2, [60.0, 60.0], [0.0, 1.0])
            .unwrap();
        let enemy = w.organizations[enemy].units[0];
        w.units[bomber].struck = Some(crate::Struck {
            from: Some(Target::Unit(enemy)),
            by: 2,
            at: w.ticks,
            filter: 0,
            damage: 1.0,
        });
        w.tick();
        assert_eq!(
            w.sorties[1].job,
            Job::Escort {
                after: Some(Target::Unit(enemy))
            }
        );
        assert_eq!(w.sorties[1].target, Some(Target::Unit(enemy)));
        // Once the attack's done, it comes home.
        w.sorties[0].mana = 0.0;
        w.tick();
        w.tick();
        assert_eq!(w.sorties[1].stage, Stage::Home);
        until(&mut w, 200.0, |w| w.sorties.iter().all(|s| s.stage.over()));
        // Switched off, none goes with the next.
        w.apply(
            1,
            Command::Escort {
                building: FIELD,
                on: false,
            },
        )
        .unwrap();
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        assert_eq!(w.sorties.len(), 4);
    }
}
