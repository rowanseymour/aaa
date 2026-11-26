//! Airfields' aircraft: the stock each keeps and rebuilds, and what it
//! sends out from its runway and brings home again: air attacks with
//! their escorts, air recon, and the fighters that see off intruders.

use std::f32::consts::FRAC_PI_4;

use tgmap::{AirJob, Id, Tile, Weapon};

use crate::abilities::Rating;
use crate::combat::Aimed;
use crate::delivery::Plunge;
use crate::targets::User;
use crate::{
    Left, Notice, Piece, Projectile, Refused, Relation, State, TICK, Target, World, distance,
};

/// How far short of its target a plane starts its run in, as a share of
/// its longest weapon's range.
const STANDOFF: f32 = 1.75;

/// The least the cosine of the angle between where a plane points and its
/// target comes to for it to fire on it.
const POINTED: f32 = 0.9;

/// The least that cosine comes to for a plane to carry on with its run in.
const RUNNING: f32 = 0.5;

/// Seconds a plane flies straight ahead after its run in, at the height
/// it had, then rising back to its own.
const CLIMB_OUT: f32 = 2.0;
const CLIMB_ON: f32 = 3.0;

/// How far past a runway's start a plane has its height after taking off,
/// and the point it lands from, in the runway's lengths; and the first
/// for a runway without a length.
const CLIMB_LENGTHS: f32 = 4.0;
const CLIMB_UNSIZED: f32 = 24.0;

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
    /// the airfield's stock; or once it's making for another, that one.
    pub airfield: usize,
    pub berth: usize,
    /// Whether it's making for a building other than the one it came
    /// from, which has no place kept for it.
    elsewhere: bool,
    pub template: Id,
    /// The side it flies for, which keeps it if its airfield changes hands.
    pub owner: u32,
    /// An air attack's planes share a wave with their escort.
    pub wave: u32,
    pub job: Job,
    pub stage: Stage,
    /// The plane as a unit, by index into [`World::units`], once it's out.
    pub plane: Option<usize>,
    /// How far above the ground it flies.
    pub height: f32,
    /// The tick it took off on.
    took_off: Option<u64>,
    /// What it has left to pay its weapons' mana with.
    pub mana: f32,
    /// The thing it's after now, for an attack on a thing.
    pub target: Option<Target>,
    /// Where it is in a pass over its target.
    leg: Leg,
    /// The legs it has still to fly taking off or landing, and the path
    /// of the one it's on.
    legs: Vec<Plan>,
    path: Option<Path>,
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
    /// Flying with its wave's attack, or attacking what last hit any of
    /// them.
    Escort { after: Option<Piece> },
    /// Flying to a place and back, seeing what's on the way.
    Recon([f32; 2]),
    /// Seeing off its airfield's intruders, attacking one of them.
    Defend { on: Option<Piece> },
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
    /// At its airfield's hangar, waiting its turn on the runway.
    Waiting,
    /// Taxiing out and taking off.
    TakingOff,
    /// Off on its job.
    Out,
    /// Flying home and landing.
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

    /// Whether it's yet to finish its job.
    fn on_job(self) -> bool {
        matches!(self, Stage::Waiting | Stage::TakingOff | Stage::Out)
    }
}

/// A leg of a pass over the target, with what it flies to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Leg {
    /// Off the runway, about to start its first approach.
    Start,
    /// To the point short of the target the run in starts from.
    Approach(Plan),
    /// In at the target, coming down.
    Run(Plan),
    /// Straight ahead at the height it came out of the run at, then on
    /// rising back to its own.
    ClimbOut(Plan),
    ClimbOn(Plan),
    /// Round to a point off to one side, to come at the target again.
    Turn(Plan),
}

/// What a leg of a flight asks: where it ends, the way the plane points
/// there if it asks one, and its height and speed by then.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Plan {
    to: [f32; 2],
    heading: Option<[f32; 2]>,
    height: f32,
    speed: f32,
}

/// The way a plane flies a leg: a smooth curve from where it was through
/// the leg's point, how far along it it's come, and the height and speed
/// it started it with.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Path {
    plan: Plan,
    curve: [[f32; 2]; 4],
    length: f32,
    along: f32,
    height: f32,
    speed: f32,
}

/// How many straight pieces a path's length is measured over.
const PIECES: usize = 16;

impl Path {
    /// From `at`, pointed `facing`, at `height` and `speed`, through
    /// `plan`'s point, going no tighter than `radius` at either end.
    fn new(
        at: [f32; 2],
        facing: [f32; 2],
        height: f32,
        speed: f32,
        plan: Plan,
        radius: f32,
    ) -> Path {
        let off = distance(at, plan.to);
        let end = (plan.heading)
            .or_else(|| crate::normalised([plan.to[0] - at[0], plan.to[1] - at[1]]))
            .unwrap_or(facing);
        // A point more than 60° off its heading has it go round first.
        let towards = crate::normalised([plan.to[0] - at[0], plan.to[1] - at[1]]);
        let behind = towards.is_some_and(|w| w[0] * facing[0] + w[1] * facing[1] < RUNNING);
        let reach = match behind {
            true => (off / 3.0).max(radius),
            false => off / 3.0,
        };
        let curve = [
            at,
            [at[0] + facing[0] * reach, at[1] + facing[1] * reach],
            [plan.to[0] - end[0] * reach, plan.to[1] - end[1] * reach],
            plan.to,
        ];
        let mut length = 0.0;
        let mut last = at;
        for i in 1..=PIECES {
            let p = bezier(&curve, i as f32 / PIECES as f32);
            length += distance(last, p);
            last = p;
        }
        Path {
            plan,
            curve: match off > 1e-4 {
                true => curve,
                false => [at; 4],
            },
            length: if off > 1e-4 { length } else { 0.0 },
            along: 0.0,
            height,
            speed,
        }
    }

    /// How far through it is, from 0 to 1.
    fn share(&self) -> f32 {
        match self.length > 0.0 {
            true => (self.along / self.length).min(1.0),
            false => 1.0,
        }
    }
}

/// The point `t` of the way along the cubic curve through `c`.
fn bezier(c: &[[f32; 2]; 4], t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    [0, 1].map(|i| (0..4).map(|k| w[k] * c[k][i]).sum())
}

/// The way the cubic curve through `c` runs at `t`.
fn bezier_way(c: &[[f32; 2]; 4], t: f32) -> Option<[f32; 2]> {
    let u = 1.0 - t;
    let w = [
        -3.0 * u * u,
        3.0 * u * u - 6.0 * u * t,
        6.0 * u * t - 3.0 * t * t,
        3.0 * t * t,
    ];
    crate::normalised([0, 1].map(|i| (0..4).map(|k| w[k] * c[k][i]).sum()))
}

/// Where a runway's planes wait, the way they face there, how far they
/// taxi straight out, and where the runway starts, the way it runs and
/// its length, on the map.
struct Strip {
    hangar: [f32; 2],
    hangar_way: [f32; 2],
    hangar_length: f32,
    start: [f32; 2],
    way: [f32; 2],
    length: f32,
}

impl Strip {
    /// The point `along` the runway from its start.
    fn along(&self, along: f32) -> [f32; 2] {
        [
            self.start[0] + self.way[0] * along,
            self.start[1] + self.way[1] * along,
        ]
    }

    /// How far past its start planes climb to, and land from.
    fn climb(&self) -> f32 {
        match self.length > 0.0 {
            true => CLIMB_LENGTHS * self.length,
            false => CLIMB_UNSIZED,
        }
    }

    /// The point planes taxi straight out to from the hangar.
    fn taxied(&self) -> [f32; 2] {
        [
            self.hangar[0] + self.hangar_way[0] * self.hangar_length,
            self.hangar[1] + self.hangar_way[1] * self.hangar_length,
        ]
    }
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

    /// What sending its group on `job` from the building at `b` costs in
    /// money: an air attack or air recon.
    pub fn air_cost(&self, b: usize, job: AirJob) -> f32 {
        let money = Id::from(crate::economy::money(&self.rules.economy));
        let costs = self.airfield(b).map(|a| match job {
            AirJob::Recon => &a.recon_cost,
            _ => &a.attack_cost,
        });
        costs.and_then(|c| c.get(&money)).copied().unwrap_or(0.0)
    }

    /// The building at `b`, if it's `player`'s and can send its group on
    /// `job`: a standing airfield with a group for it, neither going up
    /// nor packing or unpacking.
    fn sending_airfield(&self, player: u32, b: usize, job: AirJob) -> Result<(), Refused> {
        let building = self.standing_for(player, b)?;
        let group = self.airfield(b).and_then(|a| a.group(job));
        if building.busy() || group.is_none() {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    /// Whether `player` may send the building at `b`'s group on `job`
    /// now, an air attack on `on` or air recon: one its side can't afford
    /// is taken, to tell it so.
    pub(crate) fn may_send(
        &self,
        player: u32,
        b: usize,
        job: AirJob,
        on: Option<Piece>,
    ) -> Result<(), Refused> {
        self.sending_airfield(player, b, job)?;
        if on.is_some_and(|p| !self.piece_exists(p)) {
            return Err(Refused::Unable);
        }
        if !self.affords(player, b, job) {
            return Ok(());
        }
        if !self.planes_ready(b, job) {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    fn affords(&self, player: u32, b: usize, job: AirJob) -> bool {
        self.air_cost(b, job) <= self.stockpile(player)
    }

    fn piece_exists(&self, piece: Piece) -> bool {
        match piece {
            Piece::Organization(r) => r < self.organizations.len(),
            Piece::Building(b) => b < self.buildings.len(),
        }
    }

    /// Pays for the building at `b`'s group on `job`, or tells its side
    /// it can't, returning whether it was paid for.
    fn pay_for(&mut self, player: u32, b: usize, job: AirJob) -> bool {
        let cost = self.air_cost(b, job);
        if !self.affords(player, b, job) {
            (self.notices).push(Notice::AirUnaffordable {
                player,
                building: b,
                job,
                cost,
            });
            return false;
        }
        let left = self.stockpile(player) - cost;
        self.set_stockpile(player, left);
        true
    }

    /// Sends an air attack from the building at `b` on `on`, or at `at`
    /// for none, paying for it; or tells its side it can't afford one.
    pub(crate) fn order_air_attack(
        &mut self,
        player: u32,
        b: usize,
        at: [f32; 2],
        on: Option<Piece>,
    ) -> Result<(), Refused> {
        self.may_send(player, b, AirJob::Offensive, on)?;
        if !self.pay_for(player, b, AirJob::Offensive) {
            return Ok(());
        }
        let aim = match on {
            Some(piece) => AirAim::Thing(piece),
            None => AirAim::Place(self.pathing.inside(at)),
        };
        let wave = self.next_wave;
        self.next_wave += 1;
        for berth in self.ready_for(b, AirJob::Offensive) {
            self.send_out(b, berth, wave, Job::Attack(aim), [0.0; 2]);
        }
        // Its escort, if that's on and ready, takes off after it.
        self.send_escorts(b);
        Ok(())
    }

    /// Sends air recon from the building at `b` to `at`, paying for it; or
    /// tells its side it can't afford it.
    pub(crate) fn order_air_recon(
        &mut self,
        player: u32,
        b: usize,
        at: [f32; 2],
    ) -> Result<(), Refused> {
        self.may_send(player, b, AirJob::Recon, None)?;
        if !self.pay_for(player, b, AirJob::Recon) {
            return Ok(());
        }
        let at = self.pathing.inside(at);
        let wave = self.next_wave;
        self.next_wave += 1;
        for berth in self.ready_for(b, AirJob::Recon) {
            self.send_out(b, berth, wave, Job::Recon(at), [0.0; 2]);
        }
        Ok(())
    }

    /// Sets the building at `b`'s escort on or off.
    pub(crate) fn set_escort(&mut self, player: u32, b: usize, on: bool) -> Result<(), Refused> {
        self.may_set_escort(player, b)?;
        self.buildings[b].escort = on;
        if !on {
            // What's escorting leaves its attack, which may take another
            // once it's switched on again, and comes home; what's yet to
            // take off stays.
            for s in 0..self.sorties.len() {
                let sortie = &self.sorties[s];
                if sortie.airfield != b || !matches!(sortie.job, Job::Escort { .. }) {
                    continue;
                }
                self.sorties[s].wave = self.next_wave;
                self.next_wave += 1;
                match self.sorties[s].stage {
                    Stage::Waiting => self.stay_home(s),
                    _ => self.head_home(s),
                }
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

    /// Sends the building at `b`'s escort group with each of its air
    /// attacks still out that has had none, while its escort is on and
    /// the group is ready.
    fn send_escorts(&mut self, b: usize) {
        if !self.buildings[b].escort || !self.buildings[b].standing() {
            return;
        }
        let mut waves: Vec<u32> = (self.sorties.iter())
            .filter(|o| o.airfield == b && o.stage.on_job() && matches!(o.job, Job::Attack(_)))
            .map(|o| o.wave)
            .collect();
        waves.sort();
        waves.dedup();
        let slots = (self.airfield(b))
            .and_then(|a| a.group(AirJob::Escort))
            .map(|g| g.formation.clone())
            .unwrap_or_default();
        for wave in waves {
            let escorted = (self.sorties.iter())
                .any(|o| o.wave == wave && matches!(o.job, Job::Escort { .. }));
            if escorted {
                continue;
            }
            let escort = self.ready_for(b, AirJob::Escort);
            if escort.is_empty() {
                return;
            }
            for (i, berth) in escort.into_iter().enumerate() {
                let [x, y, _] = slots.get(i).copied().unwrap_or([0.0; 3]);
                self.send_out(b, berth, wave, Job::Escort { after: None }, [x, y]);
            }
        }
    }

    /// Puts the plane in the building at `b`'s place `berth` at its
    /// hangar, to wait its turn on the runway.
    fn send_out(&mut self, b: usize, berth: usize, wave: u32, job: Job, slot: [f32; 2]) {
        let strip = self.strip(b);
        let building = &self.buildings[b];
        let (owner, place) = (building.owner, &building.hangar[berth]);
        let (template, health) = (place.template.clone(), place.health);
        let s = self.sorties.len();
        let raised = self.raise(&template, owner, strip.hangar, strip.hangar_way);
        let plane = raised.map(|r| self.organizations[r].units[0]);
        let taxi = self.taxi(&template);
        let mut mana = 0.0;
        if let Some(u) = plane {
            let health = self.max_health(u) * health;
            let unit = &mut self.units[u];
            (unit.aloft, unit.health) = (true, health);
            mana = unit.fighter.as_ref().map_or(0.0, |f| f.mana);
        }
        self.sorties.push(Sortie {
            airfield: b,
            berth,
            elsewhere: false,
            template,
            owner,
            wave,
            job,
            stage: match plane {
                Some(_) => Stage::Waiting,
                None => Stage::Lost(self.ticks),
            },
            plane,
            height: taxi.taxi_height,
            took_off: None,
            mana,
            target: None,
            leg: Leg::Start,
            legs: Vec::new(),
            path: None,
            side: 1.0,
            slot,
            speed: 0.0,
            plunge: None,
        });
        if plane.is_some() {
            self.buildings[b].hangar[berth].out = Some(s);
        }
    }

    /// Takes the plane of sortie `s`, still waiting, off the map and back
    /// into its place.
    fn stay_home(&mut self, s: usize) {
        let ticks = self.ticks;
        let sortie = &mut self.sorties[s];
        sortie.stage = Stage::Landed(ticks);
        if let Some(u) = sortie.plane {
            let unit = &mut self.units[u];
            (unit.aloft, unit.left) = (false, Some(Left::Disbanded(ticks)));
        }
        let (b, berth) = (sortie.airfield, sortie.berth);
        self.buildings[b].hangar[berth].out = None;
    }

    /// The building at `b`'s runway and hangar on the map.
    fn strip(&self, b: usize) -> Strip {
        let building = &self.buildings[b];
        let runway = self.airfield(b).map(|a| a.runway).unwrap_or_default();
        let (at, facing) = (building.position, building.facing);
        let way = |w: [f32; 2]| {
            crate::normalised(tgmap::from_local([0.0; 2], facing, w)).unwrap_or(facing)
        };
        Strip {
            hangar: tgmap::from_local(at, facing, runway.hangar_offset),
            hangar_way: way(runway.hangar_orientation),
            hangar_length: runway.hangar_length,
            start: tgmap::from_local(at, facing, runway.offset),
            way: way(runway.orientation),
            length: runway.length,
        }
    }

    /// How planes of `template` are on the ground.
    fn taxi(&self, template: &Id) -> tgmap::Aircraft {
        (self.rules.aircraft.get(template).copied()).unwrap_or(tgmap::Aircraft {
            height: 0.0,
            taxi_height: 0.0,
            taxi_speed: 1.0,
            taxi_scale: 1.0,
            turn_radius: 0.0,
        })
    }

    /// Notices each airfield's intruders, sends what goes out by itself
    /// and launches the next plane waiting on each runway, then flies on
    /// and brings home or down each plane out.
    pub(crate) fn fly_sorties(&mut self) {
        for b in 0..self.buildings.len() {
            if self.buildings[b].hangar.is_empty() {
                continue;
            }
            self.watch_for_intruders(b);
            self.send_defenders(b);
            self.send_escorts(b);
            self.launch_next(b);
        }
        for s in 0..self.sorties.len() {
            let shot = (self.sorties[s].plane).is_some_and(|u| self.units[u].died.is_some());
            match self.sorties[s].stage {
                Stage::Waiting | Stage::TakingOff | Stage::Out | Stage::Home if shot => {
                    self.sortie_shot_down(s)
                }
                Stage::Waiting => {
                    // Gone with an airfield gone before it took off.
                    if !self.buildings[self.sorties[s].airfield].standing() {
                        self.lose_plane(s);
                    }
                }
                Stage::TakingOff | Stage::Out | Stage::Home => self.fly_sortie(s),
                Stage::Falling => self.fall_sortie(s),
                _ => {}
            }
        }
    }

    /// Sends the first plane waiting at the building at `b` down its
    /// runway, once its delay has passed since the last took off.
    fn launch_next(&mut self, b: usize) {
        let delay = self.airfield(b).map_or(0.0, |a| a.runway.delay);
        if delay <= 0.0 || !self.buildings[b].standing() {
            return;
        }
        let delay = (delay / TICK).round() as u64;
        let last = (self.sorties.iter())
            .filter(|o| o.airfield == b && !o.elsewhere)
            .filter_map(|o| o.took_off)
            .max();
        if last.is_some_and(|t| self.ticks < t + delay) {
            return;
        }
        let alive = |o: &Sortie| o.plane.is_some_and(|u| self.units[u].died.is_none());
        let Some(s) = (self.sorties.iter())
            .position(|o| o.airfield == b && o.stage == Stage::Waiting && alive(o))
        else {
            return;
        };
        let strip = self.strip(b);
        let template = self.sorties[s].template.clone();
        let taxi = self.taxi(&template);
        let full = self.sorties[s].plane.map_or(0.0, |u| self.flying_speed(u));
        let ground = |to, way, speed| Plan {
            to,
            heading: Some(way),
            height: taxi.taxi_height,
            speed,
        };
        let mut legs = Vec::new();
        if strip.hangar_length > 0.0 {
            legs.push(ground(strip.taxied(), strip.hangar_way, taxi.taxi_speed));
        }
        if distance(strip.hangar, strip.start) > 1e-4 {
            legs.push(ground(strip.start, strip.way, taxi.taxi_speed));
        }
        if strip.length > 0.0 {
            legs.push(ground(strip.along(strip.length), strip.way, full));
        }
        legs.push(Plan {
            to: strip.along(strip.climb()),
            heading: Some(strip.way),
            height: taxi.height,
            speed: full,
        });
        let sortie = &mut self.sorties[s];
        (sortie.stage, sortie.took_off, sortie.legs) = (Stage::TakingOff, Some(self.ticks), legs);
        sortie.speed = taxi.taxi_speed;
        log::debug!("{template} took off from building {b}");
    }

    /// How fast the plane at `u` flies: its top speed as its properties
    /// change it.
    fn flying_speed(&self, u: usize) -> f32 {
        self.top_speed(u, 0.0)
    }

    /// How far above the ground planes of `template` fly.
    fn operational_height(&self, template: &Id) -> f32 {
        self.rules.aircraft.get(template).map_or(0.0, |a| a.height)
    }

    /// Takes sortie `s` up or down a tick towards `height`, at most `rate`
    /// a second.
    fn towards_height(&mut self, s: usize, height: f32, rate: f32) {
        let sortie = &mut self.sorties[s];
        let step = (height - sortie.height).clamp(-rate * TICK, rate * TICK);
        sortie.height += step;
    }

    /// Flies the plane of sortie `s`, at `u`, a tick along its path for
    /// `plan`, laying one from where it is if it's on none for it;
    /// whether it's at the end of it.
    fn fly_plan(&mut self, s: usize, u: usize, plan: Plan) -> bool {
        let radius = self.turn_radius(u);
        let unit = &self.units[u];
        let (at, facing) = (unit.position, unit.facing);
        let sortie = &mut self.sorties[s];
        let mut path = match sortie.path {
            Some(p) if p.plan == plan => p,
            _ => Path::new(at, facing, sortie.height, sortie.speed, plan, radius),
        };
        let speed = path.speed + (plan.speed - path.speed) * path.share();
        path.along += speed.max(f32::MIN_POSITIVE) * TICK;
        let t = path.share();
        let (on, way) = match t >= 1.0 {
            true => (plan.to, plan.heading.or(bezier_way(&path.curve, 1.0))),
            false => (bezier(&path.curve, t), bezier_way(&path.curve, t)),
        };
        sortie.height = path.height + (plan.height - path.height) * t;
        sortie.speed = path.speed + (plan.speed - path.speed) * t;
        sortie.path = (t < 1.0).then_some(path);
        let speed = sortie.speed;
        let on = self.pathing.inside(on);
        let unit = &mut self.units[u];
        (unit.previous, unit.position) = (at, on);
        unit.facing = way.unwrap_or(facing);
        unit.speed = speed;
        t >= 1.0
    }

    /// Flies sortie `s`'s plane at `u` a tick along the legs it has left,
    /// whether it has none left.
    fn fly_legs(&mut self, s: usize, u: usize) -> bool {
        let Some(&plan) = self.sorties[s].legs.first() else {
            return true;
        };
        if self.fly_plan(s, u, plan) {
            self.sorties[s].legs.remove(0);
        }
        self.sorties[s].legs.is_empty()
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
            false => std::f32::consts::PI,
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

    /// The thing sortie `s`'s plane, at `u`, attacks of `piece`: the one
    /// it's after while it still can, else for a regiment the nearest of
    /// its things it can attack; none once there's nothing left to attack.
    fn target_in(&self, s: usize, u: usize, piece: Piece) -> Option<Target> {
        let of = |t: Target| match (t, piece) {
            (Target::Unit(v), Piece::Organization(r)) => self.units[v].organization == r,
            (Target::Building(x), Piece::Building(b)) => x == b,
            _ => false,
        };
        let kept = self.sorties[s].target.filter(|&t| of(t));
        if let Some(t) = kept.filter(|&t| self.can_attack(s, u, t)) {
            return Some(t);
        }
        let from = self.units[u].position;
        match piece {
            Piece::Building(b) => Some(Target::Building(b)).filter(|&t| self.can_attack(s, u, t)),
            Piece::Organization(r) => (self.survivors(r))
                .map(Target::Unit)
                .filter(|&t| self.can_attack(s, u, t))
                .min_by(|&a, &b| self.reach(from, a).total_cmp(&self.reach(from, b))),
        }
    }

    /// What sortie `s` attacks now, for an attack on a thing, keeping it
    /// as its target.
    fn pick_target(&mut self, s: usize, u: usize) -> Option<Target> {
        let piece = match self.sorties[s].job {
            Job::Attack(AirAim::Thing(piece)) => Some(piece),
            Job::Escort { after } => after,
            Job::Defend { on } => on,
            Job::Attack(AirAim::Place(_)) | Job::Recon(_) => None,
        };
        let found = piece.and_then(|p| self.target_in(s, u, p));
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
        match self.sorties[s].stage {
            Stage::TakingOff => {
                if self.fly_legs(s, u) {
                    let sortie = &mut self.sorties[s];
                    (sortie.stage, sortie.leg, sortie.path) = (Stage::Out, Leg::Start, None);
                }
                return;
            }
            Stage::Home => return self.fly_home(s, u),
            _ => {}
        }
        self.sorties[s].speed = self.flying_speed(u);
        match self.sorties[s].job {
            Job::Attack(_) => self.fly_attack(s, u),
            Job::Escort { .. } => self.fly_escort(s, u),
            Job::Recon(at) => self.fly_recon(s, u, at),
            Job::Defend { .. } => self.fly_defence(s, u),
        }
    }

    /// Turns sortie `s` for home and flies it there a tick.
    fn go_home(&mut self, s: usize, u: usize) {
        self.head_home(s);
        if self.sorties[s].stage == Stage::Home && self.units[u].alive() {
            self.fly_home(s, u);
        }
    }

    /// A tick of an attack: passes over the target until it's gone or the
    /// plane's load is spent, then home.
    fn fly_attack(&mut self, s: usize, u: usize) {
        let target = self.pick_target(s, u);
        let on_thing = !matches!(self.sorties[s].job, Job::Attack(AirAim::Place(_)));
        if on_thing && target.is_none() || !self.has_load(s, u) {
            return self.go_home(s, u);
        }
        if self.attack_tick(s, u).is_none() {
            self.go_home(s, u);
        }
    }

    /// A tick of going after sortie `s`'s target: straight after a plane
    /// in the air, else a tick of a pass. Whether a pass ended, or none
    /// for no target to go after.
    fn attack_tick(&mut self, s: usize, u: usize) -> Option<bool> {
        if let Some(Target::Unit(v)) = self.sorties[s].target
            && self.units[v].in_air()
        {
            self.chase_plane(s, u, v);
            return Some(false);
        }
        let aim = self.aim_point(s, u)?;
        Some(self.make_pass(s, u, aim))
    }

    /// A tick of flying straight after the plane at `v`, at the height it
    /// flies at.
    fn chase_plane(&mut self, s: usize, u: usize, v: usize) {
        let speed = self.sorties[s].speed;
        let height = self.operational_height(&self.units[v].template);
        self.towards_height(s, height, speed);
        self.sorties[s].path = None;
        let (at, facing) = (self.units[u].position, self.units[u].facing);
        let there = self.units[v].position;
        let want = crate::normalised([there[0] - at[0], there[1] - at[1]]).unwrap_or(facing);
        let most = self.turning(u, speed);
        self.steer(u, want, most, speed);
    }

    /// A tick of a pass over `aim`: an approach to a point short of it at
    /// the plane's height, a run in at it coming down, a climb out
    /// straight ahead, and a turn away to one side to come round again.
    /// Whether the pass ended with this tick.
    fn make_pass(&mut self, s: usize, u: usize, aim: [f32; 2]) -> bool {
        let (_, longest) = self.plane_weapons(u);
        let standoff = STANDOFF * longest;
        let full = self.operational_height(&self.sorties[s].template);
        let half = full / 2.0;
        let speed = self.sorties[s].speed;
        let towards_from = |at: [f32; 2], facing| {
            crate::normalised([aim[0] - at[0], aim[1] - at[1]]).unwrap_or(facing)
        };
        let back = |way: [f32; 2]| [aim[0] - way[0] * standoff, aim[1] - way[1] * standoff];
        let plan = |to, heading, height| Plan {
            to,
            heading,
            height,
            speed,
        };
        let leg = self.sorties[s].leg;
        let flown = match leg {
            Leg::Start => None,
            Leg::Approach(p) | Leg::Run(p) | Leg::ClimbOut(p) | Leg::ClimbOn(p) | Leg::Turn(p) => {
                Some(self.fly_plan(s, u, p))
            }
        };
        let unit = &self.units[u];
        let (at, facing) = (unit.position, unit.facing);
        let towards = towards_from(at, facing);
        let ahead = facing[0] * towards[0] + facing[1] * towards[1];
        let done = flown == Some(true);
        let height = self.sorties[s].height;
        let straight = |seconds: f32, height| {
            let to = [
                at[0] + facing[0] * speed * seconds,
                at[1] + facing[1] * speed * seconds,
            ];
            plan(to, Some(facing), height)
        };
        let sortie = &mut self.sorties[s];
        // The legs that bring it round to the target arrive pointed at it.
        let at_target = |to: [f32; 2]| crate::normalised([aim[0] - to[0], aim[1] - to[1]]);
        let next = match leg {
            Leg::Start => Some(Leg::Approach(plan(back(towards), Some(towards), full))),
            Leg::Approach(_) if done => Some(Leg::Run(plan(aim, None, -half))),
            // Coming down along the line to a point half its height below
            // the ground there.
            Leg::Run(_) if done || height <= half || ahead < RUNNING => {
                Some(Leg::ClimbOut(straight(CLIMB_OUT, height)))
            }
            Leg::ClimbOut(_) if done => Some(Leg::ClimbOn(straight(CLIMB_ON, full))),
            Leg::ClimbOn(_) if done => {
                let turned = back(rotated(towards, sortie.side * FRAC_PI_4));
                sortie.side = -sortie.side;
                Some(Leg::Turn(plan(turned, at_target(turned), full)))
            }
            Leg::Turn(_) if done => Some(Leg::Approach(plan(back(towards), Some(towards), full))),
            _ => None,
        };
        let ended = matches!(leg, Leg::Turn(_)) && done;
        if let Some(next) = next {
            (sortie.leg, sortie.path) = (next, None);
            if flown.is_none()
                && let Leg::Approach(p) = next
            {
                self.fly_plan(s, u, p);
            }
        }
        ended
    }

    /// A tick of an escort: with its wave's attack, or attacking what last
    /// hit one of them, until the attack's done.
    fn fly_escort(&mut self, s: usize, u: usize) {
        let wave = self.sorties[s].wave;
        let attacking = |o: &Sortie| o.wave == wave && matches!(o.job, Job::Attack(_));
        let attacks: Vec<usize> = (0..self.sorties.len())
            .filter(|&o| attacking(&self.sorties[o]))
            .collect();
        if !attacks.iter().any(|&o| self.sorties[o].stage.on_job()) {
            return self.go_home(s, u);
        }
        // What most lately hit any of the wave's planes, if an enemy.
        let wave_planes = (0..self.sorties.len())
            .filter(|&o| self.sorties[o].wave == wave && !self.sorties[o].stage.over())
            .filter_map(|o| self.sorties[o].plane);
        let owner = self.sorties[s].owner;
        let hit = (wave_planes.filter_map(|p| self.units[p].struck))
            .filter(|h| self.sides.relation(h.by, owner) == Relation::Enemy)
            .max_by_key(|h| h.at)
            .and_then(|h| h.from)
            .filter(|&t| self.can_attack(s, u, t));
        if let Some(t) = hit {
            let piece = self.piece(t);
            if self.sorties[s].job != (Job::Escort { after: Some(piece) }) {
                self.sorties[s].job = Job::Escort { after: Some(piece) };
                self.sorties[s].target = Some(t);
            }
        }
        if self.pick_target(s, u).is_some() && self.attack_tick(s, u).is_some() {
            return;
        }
        let sortie = &mut self.sorties[s];
        (sortie.job, sortie.target, sortie.leg) = (Job::Escort { after: None }, None, Leg::Start);
        sortie.path = None;
        // In its place beside the attack's first plane still out.
        let lead = (attacks.iter())
            .filter(|&&o| self.sorties[o].stage == Stage::Out)
            .find_map(|&o| self.sorties[o].plane);
        let speed = self.sorties[s].speed;
        let full = self.operational_height(&self.sorties[s].template);
        self.towards_height(s, full, speed);
        let Some(lead) = lead else {
            // Waiting on the attack to take off.
            let facing = self.units[u].facing;
            return self.steer(u, facing, 0.0, speed);
        };
        let (lead_at, lead_facing) = (self.units[lead].position, self.units[lead].facing);
        let slot = tgmap::from_local(lead_at, lead_facing, self.sorties[s].slot);
        let at = self.units[u].position;
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

    /// A tick of air recon: to the point at its height, then straight
    /// home.
    fn fly_recon(&mut self, s: usize, u: usize, to: [f32; 2]) {
        let plan = Plan {
            to,
            heading: None,
            height: self.operational_height(&self.sorties[s].template),
            speed: self.sorties[s].speed,
        };
        if self.fly_plan(s, u, plan) {
            self.head_home(s);
        }
    }

    /// A tick of a defender: attacking the intruder it's fighting while
    /// that's still one, else the nearest it can attack; home with none
    /// left, its airfield gone, or once it's out of shots and has
    /// finished its pass.
    fn fly_defence(&mut self, s: usize, u: usize) {
        let b = self.sorties[s].airfield;
        let intruders = match self.buildings[b].standing() {
            true => self.buildings[b].intruders.clone(),
            false => Vec::new(),
        };
        let on = match self.sorties[s].job {
            Job::Defend { on } => on.filter(|p| intruders.contains(p)),
            _ => None,
        };
        let kept = on.filter(|&p| self.target_in(s, u, p).is_some());
        let on = kept.or_else(|| {
            let from = self.units[u].position;
            (intruders.iter().copied())
                .filter_map(|p| Some((p, self.target_in(s, u, p)?)))
                .min_by(|(_, a), (_, c)| {
                    distance(from, self.position(*a)).total_cmp(&distance(from, self.position(*c)))
                })
                .map(|(p, _)| p)
        });
        self.sorties[s].job = Job::Defend { on };
        if on.is_none() || self.pick_target(s, u).is_none() {
            return self.go_home(s, u);
        }
        let loaded = self.has_load(s, u);
        let chasing = !loaded
            && matches!(self.sorties[s].target, Some(Target::Unit(v)) if self.units[v].in_air());
        if chasing {
            return self.go_home(s, u);
        }
        match self.attack_tick(s, u) {
            Some(ended) if loaded || !ended => {}
            _ => self.go_home(s, u),
        }
    }

    /// Whether `piece` is still an intruder of the building at `b`: an
    /// enemy of its side's that can be targeted, within its guard
    /// operational range and, for a regiment, with some of its units
    /// still with it.
    fn still_intruding(&self, b: usize, piece: Piece) -> bool {
        let building = &self.buildings[b];
        let range = self.airfield(b).map_or(0.0, |a| a.guard_operational_range);
        if !self.piece_exists(piece)
            || self.sides.relation(building.owner, self.piece_owner(piece)) != Relation::Enemy
        {
            return false;
        }
        let near = |t: Target| distance(building.position, self.position(t)) < range;
        match piece {
            Piece::Building(x) => {
                let t = Target::Building(x);
                self.fighter(t).is_some() && near(t)
            }
            Piece::Organization(r) => {
                let from = self.leader_position(r);
                let units: Vec<Target> = (self.survivors(r))
                    .map(Target::Unit)
                    .filter(|&t| self.fighter(t).is_some())
                    .collect();
                let with = |t: &Target| matches!(*t, Target::Unit(v) if !self.strays(r, v, from));
                units.iter().any(with) && units.iter().copied().any(near)
            }
        }
    }

    /// Forgets the building at `b`'s intruders that no longer are, and
    /// notices enemies its side sees coming into its guard zone.
    fn watch_for_intruders(&mut self, b: usize) {
        let defends = self.airfield(b).and_then(|a| a.group(AirJob::Defensive));
        if defends.is_none() || !self.buildings[b].standing() {
            let building = &mut self.buildings[b];
            (building.intruders, building.guarding) = (Vec::new(), false);
            return;
        }
        let range = self.airfield(b).map_or(0.0, |a| a.guard_range);
        let mut intruders = std::mem::take(&mut self.buildings[b].intruders);
        intruders.retain(|&p| self.still_intruding(b, p));
        let building = &self.buildings[b];
        // Only lay the zone with an enemy near enough to be in it.
        let owner = building.owner;
        let near = self.targets().any(|t| {
            self.sides.relation(owner, self.owner(t)) == Relation::Enemy
                && self.reach(building.position, t) <= range + 2.0 * tgmap::pathing::CELL_SIZE
        });
        if !near {
            let building = &mut self.buildings[b];
            (building.intruders, building.guarding) = (intruders, true);
            return;
        }
        let zone = self.pathing.zone(building.position, range);
        // What's there as the zone's laid comes in with it.
        let laid = !building.guarding;
        let owner = building.owner;
        let came = |t: Target| match t {
            Target::Unit(v) => {
                let unit = &self.units[v];
                laid || Tile::at(unit.previous) != Tile::at(unit.position)
            }
            Target::Building(_) => laid,
        };
        let seen = |t: Target| match t {
            Target::Unit(v) => self.sees_unit(owner, v),
            Target::Building(x) => self.sees_building(owner, x),
        };
        let noticed = self.targets().filter(|&t| {
            self.sides.relation(owner, self.owner(t)) == Relation::Enemy
                && self.in_zone(&zone, t)
                && came(t)
                && seen(t)
        });
        for t in noticed.collect::<Vec<_>>() {
            let piece = self.piece(t);
            if !intruders.contains(&piece) && self.still_intruding(b, piece) {
                intruders.push(piece);
            }
        }
        let building = &mut self.buildings[b];
        (building.intruders, building.guarding) = (intruders, true);
    }

    /// Sends out the building at `b`'s defensive group, free, again and
    /// again while it has intruders and the group's planes are ready.
    fn send_defenders(&mut self, b: usize) {
        let building = &self.buildings[b];
        if building.intruders.is_empty() || !building.standing() || building.busy() {
            return;
        }
        loop {
            let defenders = self.ready_for(b, AirJob::Defensive);
            if defenders.is_empty() {
                return;
            }
            let wave = self.next_wave;
            self.next_wave += 1;
            for &berth in &defenders {
                self.send_out(b, berth, wave, Job::Defend { on: None }, [0.0; 2]);
            }
            // What couldn't be put on the map stays ready; don't try again.
            if defenders
                .iter()
                .all(|&p| self.buildings[b].hangar[p].out.is_none())
            {
                return;
            }
        }
    }

    /// Whether the building at `b` is gone for good, leaving its planes
    /// nowhere to come home to.
    fn airfield_gone(&self, b: usize) -> bool {
        matches!(
            self.buildings[b].state,
            State::Destroyed(_) | State::Sold(_)
        )
    }

    /// Turns sortie `s` for home, on the legs that bring it in to land.
    fn head_home(&mut self, s: usize) {
        let sortie = &mut self.sorties[s];
        if sortie.stage != Stage::Out {
            return;
        }
        (sortie.stage, sortie.target, sortie.path) = (Stage::Home, None, None);
        self.find_home(s);
    }

    /// Lays sortie `s`'s legs in to land at its airfield; or with that
    /// gone, at the nearest of its side's buildings with places for it,
    /// and with none, or its airfield packed into a truck with no runway,
    /// has it die where it is.
    fn find_home(&mut self, s: usize) {
        let sortie = &self.sorties[s];
        let b = sortie.airfield;
        let home = match self.buildings[b].state {
            State::Packed(..) => None,
            _ if self.airfield_gone(b) => self.another_home(s),
            _ => Some(b),
        };
        let Some(home) = home else {
            return self.lose_plane(s);
        };
        if home != b {
            let sortie = &mut self.sorties[s];
            (sortie.airfield, sortie.elsewhere) = (home, true);
            log::debug!("{} making for building {home} instead", sortie.template);
        }
        let strip = self.strip(home);
        let template = self.sorties[s].template.clone();
        let taxi = self.taxi(&template);
        let full = self.sorties[s].speed;
        let against = |w: [f32; 2]| [-w[0], -w[1]];
        let ground = |to, way, speed| Plan {
            to,
            heading: Some(against(way)),
            height: taxi.taxi_height,
            speed,
        };
        let mut legs = vec![
            Plan {
                to: strip.along(strip.climb()),
                heading: Some(against(strip.way)),
                height: taxi.height,
                speed: full,
            },
            ground(strip.along(strip.length), strip.way, full),
        ];
        if strip.length > 0.0 {
            legs.push(ground(strip.start, strip.way, taxi.taxi_speed));
        }
        if strip.hangar_length > 0.0 {
            legs.push(ground(strip.taxied(), strip.hangar_way, taxi.taxi_speed));
            legs.push(ground(strip.hangar, strip.hangar_way, taxi.taxi_speed));
        }
        self.sorties[s].legs = legs;
    }

    /// The nearest standing building of sortie `s`'s side, other than the
    /// one it's making for, whose stock has places for its kind of plane.
    fn another_home(&self, s: usize) -> Option<usize> {
        let sortie = &self.sorties[s];
        let at = sortie.plane.map_or([0.0; 2], |u| self.units[u].position);
        (0..self.buildings.len())
            .filter(|&b| b != sortie.airfield)
            .filter(|&b| {
                let building = &self.buildings[b];
                building.standing()
                    && building.owner == sortie.owner
                    && building
                        .hangar
                        .iter()
                        .any(|p| p.template == sortie.template)
            })
            .min_by(|&a, &c| {
                let d = |b: usize| distance(at, self.buildings[b].position);
                d(a).total_cmp(&d(c))
            })
    }

    /// Has the plane of sortie `s`, with nowhere to come home to, die
    /// where it is.
    fn lose_plane(&mut self, s: usize) {
        let sortie = &self.sorties[s];
        let Some(u) = sortie.plane.filter(|&u| self.units[u].alive()) else {
            return;
        };
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

    /// A tick of the flight home: out beyond the runway, down onto it and
    /// back along it, landing at the end. One whose home has gone looks
    /// for another.
    fn fly_home(&mut self, s: usize, u: usize) {
        let b = self.sorties[s].airfield;
        let packed = matches!(self.buildings[b].state, State::Packed(..));
        if packed || self.airfield_gone(b) {
            self.sorties[s].path = None;
            self.find_home(s);
            if !self.units[u].alive() {
                return;
            }
        }
        if self.fly_legs(s, u) {
            self.land_sortie(s, u);
        }
    }

    /// Brings the plane of sortie `s` back into its place in its
    /// airfield's stock, keeping its health; at another building, into
    /// one of its empty places of its kind, or off the map with none.
    fn land_sortie(&mut self, s: usize, u: usize) {
        let ticks = self.ticks;
        let health = self.units[u].health / self.max_health(u).max(f32::MIN_POSITIVE);
        let unit = &mut self.units[u];
        (unit.aloft, unit.left) = (false, Some(Left::Disbanded(ticks)));
        let sortie = &self.sorties[s];
        let b = sortie.airfield;
        let hangar = &self.buildings[b].hangar;
        let berth = match sortie.elsewhere {
            false => Some(sortie.berth),
            true => (hangar.iter())
                .position(|p| p.template == sortie.template && p.out.is_none() && p.health <= 0.0),
        };
        self.sorties[s].stage = match berth {
            Some(_) => Stage::Landed(ticks),
            None => Stage::Lost(ticks),
        };
        if let Some(berth) = berth {
            let place = &mut self.buildings[b].hangar[berth];
            (place.out, place.health) = (None, health.clamp(0.0, 1.0));
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
        // One making for another building has no place there.
        let (b, berth, elsewhere) = (sortie.airfield, sortie.berth, sortie.elsewhere);
        if !elsewhere {
            let place = &mut self.buildings[b].hangar[berth];
            (place.out, place.health) = (None, 0.0);
        }
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

    /// Has each plane out after a target drop on, or fire at, it whenever
    /// it's pointed at it and within reach, with the ability it would
    /// choose against it.
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
            let sortie = &self.sorties[s];
            let aimed = match (sortie.job, sortie.target) {
                (Job::Attack(AirAim::Place(at)), _) => Aimed::Ground(at),
                (_, Some(t)) => Aimed::Thing(t),
                _ => continue,
            };
            let unit = &self.units[u];
            let (at, facing) = (unit.position, unit.facing);
            let towards = self.aimed_point(aimed);
            let Some(way) = crate::normalised([towards[0] - at[0], towards[1] - at[1]]) else {
                continue;
            };
            if facing[0] * way[0] + facing[1] * way[1] < POINTED {
                continue;
            }
            let Some(w) = self.air_weapon(s, u, aimed) else {
                continue;
            };
            let cost = self.units[u]
                .fighter
                .as_ref()
                .map_or(0.0, |f| f.weapons[w].mana_cost);
            self.sorties[s].mana -= cost;
            log::debug!(
                "{} {} used {w} on {aimed:?}, {} mana left",
                self.units[u].template,
                self.units[u].id,
                self.sorties[s].mana
            );
            self.fire(u, w, aimed, hits, launched);
        }
    }

    /// The weapon the plane at `u` of sortie `s` uses on `aimed` now, if
    /// any is ready, in reach and paid for: on a thing, the one it would
    /// choose against it; on the ground, the best scoring that hits an
    /// area.
    fn air_weapon(&mut self, s: usize, u: usize, aimed: Aimed) -> Option<usize> {
        let mana = self.sorties[s].mana;
        let fighter = self.units[u].fighter.as_ref()?;
        let payable = |w: usize| fighter.weapons[w].mana_cost <= mana;
        let ranges = self.ranges(u);
        match aimed {
            Aimed::Thing(t) => {
                let mut rng = self.rng;
                let pick = self.choose(u, &ranges, t, &mut rng);
                self.rng = rng;
                pick.filter(|p| p.rating == Rating::UseNow && payable(p.weapon))
                    .map(|p| p.weapon)
            }
            Aimed::Ground(at) => {
                let off = distance(self.units[u].position, at);
                (fighter.weapons.iter().enumerate())
                    .filter(|&(w, weapon)| {
                        self.has(u, weapon)
                            && weapon.area > 0.0
                            && off <= ranges[w]
                            && self.ready(u, w)
                            && payable(w)
                    })
                    .map(|(w, _)| (w, self.score_at(u, w, Rating::UseNow, None)))
                    .fold(None, |best: Option<(usize, f32)>, (w, score)| match best {
                        Some((_, b)) if b >= score => best,
                        _ => Some((w, score)),
                    })
                    .map(|(w, _)| w)
            }
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

    /// What sortie `s`'s plane is drawn at, of its size: smaller near the
    /// ground while it's waiting, taking off, landing or on a run in.
    pub fn flight_scale(&self, s: usize) -> f32 {
        let sortie = &self.sorties[s];
        let low = matches!(
            (sortie.stage, sortie.leg),
            (Stage::Waiting | Stage::TakingOff | Stage::Home, _) | (Stage::Out, Leg::Run(_))
        );
        let taxi = self.taxi(&sortie.template);
        let span = taxi.height - taxi.taxi_height;
        match (low, span > 0.0) {
            (false, _) => 1.0,
            (true, false) => taxi.taxi_scale,
            (true, true) => {
                let up = ((sortie.height - taxi.taxi_height) / span).clamp(0.0, 1.0);
                taxi.taxi_scale + (1.0 - taxi.taxi_scale) * up
            }
        }
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
    elsewhere,
    template,
    owner,
    wave,
    job,
    stage,
    plane,
    height,
    took_off,
    mana,
    target,
    leg,
    legs,
    path,
    side,
    slot,
    speed,
    plunge,
});

crate::hash::digest!(Plan {
    to,
    heading,
    height,
    speed
});

crate::hash::digest!(Path {
    plan,
    curve,
    length,
    along,
    height,
    speed
});

impl crate::hash::Digest for Job {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        match *self {
            Job::Attack(aim) => (0u8, aim).digest(h),
            Job::Escort { after } => (1u8, after).digest(h),
            Job::Recon(at) => (2u8, at).digest(h),
            Job::Defend { on } => (3u8, on).digest(h),
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
            Stage::TakingOff => 7u8.digest(h),
        }
    }
}

impl crate::hash::Digest for Leg {
    fn digest(&self, h: &mut crate::hash::Hasher) {
        match *self {
            Leg::Start => 0u8.digest(h),
            Leg::Approach(p) => (1u8, p).digest(h),
            Leg::Run(p) => (2u8, p).digest(h),
            Leg::ClimbOut(p) => (3u8, p).digest(h),
            Leg::ClimbOn(p) => (4u8, p).digest(h),
            Leg::Turn(p) => (5u8, p).digest(h),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::testing::*;
    use crate::{Command, Pathing};
    use tgmap::{AirGroup, Aircraft, Airfield, Domain, Runway};

    /// The airfield's index in [`World::buildings`].
    pub(crate) const FIELD: usize = 1;

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
    /// one or two fighters, recon and defence one fighter, a runway from
    /// 5 ahead of it with a second between take-offs, an attack costing
    /// 150 and recon 25, noticing intruders within 30 and forgetting them
    /// beyond 48. Player 2 has a house at 80,50.
    pub(crate) fn field() -> World {
        let mut w = economy();
        w.pathing = Pathing::uniform(120, Domain::Land);
        let mut airfield = building(6, 1, [20.0, 50.0]);
        airfield.template = "airfield".into();
        let house = building(7, 2, [80.0, 50.0]);
        w.add_buildings(&[airfield, house]);
        let ids = |ids: &[&str]| ids.iter().map(|&i| Id::from(i)).collect::<Vec<_>>();
        let group = |template: &str, [fewest, most]: [usize; 2], jobs: &[AirJob]| AirGroup {
            templates: ids(&[template]),
            fewest,
            most,
            jobs: jobs.to_vec(),
            formation: vec![[-9.0, 15.0, -2.0], [9.0, 15.0, -2.0]],
        };
        w.rules.airfields.insert(
            Id::from("airfield"),
            Airfield {
                stock: ids(&["bomber", "bomber", "fighter", "fighter"]),
                groups: vec![
                    group("bomber", [1, 1], &[AirJob::Offensive]),
                    group("fighter", [1, 1], &[AirJob::Defensive, AirJob::Recon]),
                    group("fighter", [1, 2], &[AirJob::Escort]),
                ],
                runway: Runway {
                    offset: [0.0, 5.0],
                    orientation: [-1.0, 0.0],
                    length: 4.8,
                    hangar_offset: [2.5, 1.0],
                    hangar_orientation: [0.0, 1.0],
                    hangar_length: 2.45,
                    delay: 1.0,
                },
                attack_cost: [(Id::from("money"), 150.0)].into(),
                recon_cost: [(Id::from("money"), 25.0)].into(),
                guard_range: 30.0,
                guard_operational_range: 48.0,
            },
        );
        // The stock as the map has it.
        w.buildings[FIELD].hangar = w.hangar(&Id::from("airfield"), true);
        let mut gun = rifle(14.0, 10.0);
        (gun.recharge, gun.mana_cost) = (0.2, 1.0);
        for (template, speed, health, weapons, mana, height, radius) in [
            ("bomber", 7.0, 300.0, vec![bomb()], 60.0, 17.0, 18.0),
            ("fighter", 11.0, 100.0, vec![gun], 20.0, 15.0, 8.0),
        ] {
            let o = plane(template, speed, health, weapons, mana);
            w.rules.raisable.insert(Id::from(template), o);
            let aircraft = Aircraft {
                height,
                taxi_height: 1.0,
                taxi_speed: 3.0,
                taxi_scale: 0.4,
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
        let u = w.sorties[0].plane.expect("out");
        assert!(w.units[u].in_air() && !w.commands(1, w.units[u].organization));
        // Off the ground, up to its height, along the runway, 5 ahead of
        // the airfield, which faces -y.
        assert!(w.flight_height(u).unwrap() < 2.0);
        until(&mut w, 10.0, |w| w.flight_height(u) == Some(17.0));
        assert!((w.units[u].position[1] - 45.0).abs() < 0.01);
        assert!(near(w.units[u].facing, [1.0, 0.0]));
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
        let told = Notice::AirUnaffordable {
            player: 1,
            building: FIELD,
            job: AirJob::Offensive,
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
    fn bombs_flagged_for_bombardment_keep_what_they_hit_out_of_supply() {
        let bombed = |bombard: bool| {
            let mut w = field();
            for o in w.rules.raisable.values_mut() {
                for m in &mut o.members {
                    if let Some(f) = m.fighter.as_mut() {
                        f.weapons.iter_mut().for_each(|w| w.bombard = bombard);
                    }
                }
            }
            let mut o = truck();
            (o.template, o.unpacks, o.primary) = ("one".into(), None, Some(0));
            w.rules.raisable.insert(Id::from("one"), o);
            let r = w
                .raise(&Id::from("one"), 2, [80.0, 20.0], [0.0, 1.0])
                .unwrap();
            let u = w.organizations[r].units[0];
            w.apply(1, attack(Some(Piece::Organization(r)), [80.0, 20.0]))
                .unwrap();
            until(&mut w, 60.0, |w| w.units[u].health < 100.0);
            let plane = w.sorties[0].plane.unwrap();
            let by = Piece::Organization(w.units[plane].organization);
            (w.organizations[r].bombarded.iter().map(|b| b.by)).collect::<Vec<_>>() == [by]
        };
        assert!(bombed(true));
        assert!(!bombed(false));
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
        until(&mut w, 20.0, |w| w.sorties[0].stage == Stage::Out);
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
        until(&mut w, 20.0, |w| w.sorties[0].stage == Stage::Out);
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
        until(&mut w, 20.0, |w| {
            w.sorties.iter().all(|s| s.stage == Stage::Out)
        });
        run(&mut w, 8.0);
        let bomber = w.sorties[0].plane.unwrap();
        let escort = w.sorties[1].plane.unwrap();
        assert!(distance(w.units[bomber].position, w.units[escort].position) < 25.0);
        // An enemy hits the bomber.
        let enemy = w
            .raise(&Id::from("truck"), 2, [60.0, 60.0], [0.0, 1.0])
            .unwrap();
        let regiment = enemy;
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
                after: Some(Piece::Organization(regiment))
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

    fn escort(on: bool) -> Command {
        Command::Escort {
            building: FIELD,
            on,
        }
    }

    /// An enemy regiment of player 2's of one truck at `at`.
    fn intruder(w: &mut World, at: [f32; 2]) -> usize {
        w.raise(&Id::from("truck"), 2, at, [0.0, 1.0]).unwrap()
    }

    #[test]
    fn planes_wait_at_the_hangar_and_take_off_a_delay_apart() {
        // `airfields#plane-appears`, `airfields#runway-takeoff`
        let mut w = field();
        w.apply(1, on_house()).unwrap();
        w.apply(1, on_house()).unwrap();
        // Both at the hangar at once, drawn small; the first goes at once.
        let second = w.sorties[1].plane.unwrap();
        assert!(near(w.units[second].position, [17.5, 49.0]));
        assert_eq!(w.flight_scale(1), 0.4);
        w.tick();
        assert_eq!(w.sorties[0].stage, Stage::TakingOff);
        assert_eq!(w.sorties[1].stage, Stage::Waiting);
        until(&mut w, 2.0, |w| w.sorties[1].stage == Stage::TakingOff);
        let delay = (1.0 / TICK).round() as u64;
        let [first, second] = [0, 1].map(|s| w.sorties[s].took_off.unwrap());
        assert_eq!(second - first, delay);
    }

    #[test]
    fn a_plane_taxis_out_runs_down_the_runway_and_climbs_away() {
        // `airfields#takeoff-run`
        let mut w = field();
        w.apply(1, on_house()).unwrap();
        w.tick();
        let u = w.sorties[0].plane.unwrap();
        // Out of the hangar, onto the runway's start, along it and up to
        // its height four lengths past its start.
        let legs: Vec<_> = w.sorties[0].legs.iter().map(|p| (p.to, p.height)).collect();
        let ends = [[17.5, 46.55], [20.0, 45.0], [24.8, 45.0], [39.2, 45.0]];
        for (i, (to, height)) in legs.iter().enumerate() {
            assert!(near(*to, ends[i]), "{i}: {to:?}");
            let want = if i < 3 { 1.0 } else { 17.0 };
            assert_eq!(*height, want);
        }
        until(&mut w, 20.0, |w| w.sorties[0].stage == Stage::Out);
        assert!(near(w.units[u].position, [39.2, 45.0]));
        assert_eq!(w.flight_height(u), Some(17.0));
        assert_eq!(w.flight_scale(0), 1.0);
    }

    #[test]
    fn a_pass_approaches_runs_in_climbs_out_and_turns() {
        // `airfields#attack-passes`, `airfields#leg-path`
        let mut w = field();
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        until(&mut w, 20.0, |w| w.sorties[0].stage == Stage::Out);
        w.tick();
        assert!(matches!(w.sorties[0].leg, Leg::Approach(_)));
        until(&mut w, 60.0, |w| matches!(w.sorties[0].leg, Leg::Run(_)));
        // The run in starts right at the standoff, 1.75 times the bombs'
        // 15, at its height.
        let u = w.sorties[0].plane.unwrap();
        let off = distance(w.units[u].position, [70.0, 80.0]);
        assert!((off - 26.25).abs() < 0.5, "{off}");
        assert!((w.sorties[0].height - 17.0).abs() < 0.5);
        assert!(w.flight_scale(0) < 1.0 || w.sorties[0].height >= 17.0);
        until(&mut w, 60.0, |w| {
            matches!(w.sorties[0].leg, Leg::ClimbOut(_))
        });
        let (facing, height) = (w.units[u].facing, w.sorties[0].height);
        assert!(height <= 8.6, "{height}");
        until(&mut w, 3.0, |w| matches!(w.sorties[0].leg, Leg::ClimbOn(_)));
        assert!(near(w.units[u].facing, facing));
        assert!((w.sorties[0].height - height).abs() < 0.01);
        until(&mut w, 4.0, |w| matches!(w.sorties[0].leg, Leg::Turn(_)));
        assert!(near(w.units[u].facing, facing));
        assert_eq!(w.sorties[0].height, 17.0);
    }

    #[test]
    fn a_plane_after_a_plane_in_the_air_chases_it_at_its_height() {
        // `airfields#air-target-chase`
        let mut w = field();
        let airfield = w.rules.airfields.get_mut(&Id::from("airfield")).unwrap();
        airfield.groups[0].templates = vec![Id::from("fighter")];
        let r = w
            .raise(&Id::from("bomber"), 2, [60.0, 50.0], [0.0, 1.0])
            .unwrap();
        let enemy = w.organizations[r].units[0];
        w.units[enemy].aloft = true;
        w.apply(1, attack(Some(Piece::Organization(r)), [60.0, 50.0]))
            .unwrap();
        until(&mut w, 30.0, |w| w.units[enemy].health < 300.0);
        // No passes: still on its first leg, at the bomber's height.
        assert_eq!(w.sorties[0].leg, Leg::Start);
        assert!((w.sorties[0].height - 17.0).abs() < 0.01);
    }

    #[test]
    fn a_bomber_uses_the_bomb_combat_would_choose() {
        // `airfields#bomb-drop`: by score, not by priority alone.
        let mut w = field();
        let mut heavy = bomb();
        heavy.choice.priority = 2.0;
        let mut light = bomb();
        (light.choice.priority, light.choice.bonus.use_now) = (1.0, 100.0);
        let bomber = w.rules.raisable.get_mut(&Id::from("bomber")).unwrap();
        bomber.members[0].fighter.as_mut().unwrap().weapons = vec![heavy, light];
        w.apply(1, on_house()).unwrap();
        until(&mut w, 60.0, |w| {
            w.sorties[0].plane.is_some() && w.sorties[0].mana < 60.0
        });
        let u = w.sorties[0].plane.unwrap();
        let fired = &w.units[u].fired_ago;
        assert!(fired[0].is_infinite() && fired[1].is_finite(), "{fired:?}");
    }

    #[test]
    fn an_escort_joins_an_attack_out_once_its_ready_or_switched_on() {
        // `airfields#escort-group`
        let mut w = field();
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        run(&mut w, 3.0);
        assert_eq!(w.sorties.len(), 1);
        w.apply(1, escort(true)).unwrap();
        w.tick();
        let escorts: Vec<_> = (w.sorties.iter())
            .filter(|s| matches!(s.job, Job::Escort { .. }))
            .map(|s| s.wave)
            .collect();
        assert_eq!(escorts, [w.sorties[0].wave; 2]);
        // Not ready when ordered, it goes out once it is.
        let mut w = field();
        w.apply(1, escort(true)).unwrap();
        for berth in &mut w.buildings[FIELD].hangar[2..] {
            berth.health = 0.9;
        }
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        assert_eq!(w.sorties.len(), 1);
        until(&mut w, 5.0, |w| w.sorties.len() > 1);
        assert!(w.sorties[1..].iter().all(|s| s.wave == w.sorties[0].wave));
        // One escort an attack: no more go once it has one.
        let n = w.sorties.len();
        run(&mut w, 5.0);
        assert_eq!(w.sorties.len(), n);
    }

    #[test]
    fn an_escort_attacks_an_enemy_that_hits_its_regiment_through_and_stays_out() {
        // `airfields#escort-retaliation`
        let mut w = field();
        w.apply(1, escort(true)).unwrap();
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        run(&mut w, 6.0);
        let mut o = truck();
        (o.template, o.unpacks, o.primary) = ("pair".into(), None, None);
        let mut second = o.members[0].clone();
        second.id = 1;
        o.members.push(second);
        o.formations[0].offsets = vec![[0.0, 0.0], [0.0, 10.0]];
        w.rules.raisable.insert(Id::from("pair"), o);
        let r = w
            .raise(&Id::from("pair"), 2, [60.0, 60.0], [0.0, 1.0])
            .unwrap();
        let (first, other) = (w.organizations[r].units[0], w.organizations[r].units[1]);
        let bomber = w.sorties[0].plane.unwrap();
        let struck = |by, from| crate::Struck {
            from: Some(Target::Unit(from)),
            by,
            at: 0,
            filter: 0,
            damage: 1.0,
        };
        // Only an enemy's hit counts.
        w.units[bomber].struck = Some(struck(1, first));
        w.tick();
        assert_eq!(w.sorties[1].job, Job::Escort { after: None });
        let mut hit = struck(2, first);
        hit.at = w.ticks;
        w.units[bomber].struck = Some(hit);
        w.tick();
        let after = Job::Escort {
            after: Some(Piece::Organization(r)),
        };
        assert_eq!(w.sorties[1].job, after);
        // Its weapons spent, it stays with the attack.
        w.sorties[1].mana = 0.0;
        // As an attack on the regiment: the next nearest once one dies.
        w.units[first].die(w.ticks);
        w.tick();
        assert_eq!(w.sorties[1].target, Some(Target::Unit(other)));
        assert_eq!(w.sorties[1].stage, Stage::Out);
    }

    #[test]
    fn air_recon_costs_flies_to_the_place_and_straight_home() {
        // `airfields#air-recon-cost`, `airfields#recon-group`,
        // `airfields#recon-flight`
        let recon = |at| Command::AirRecon {
            building: FIELD,
            at,
        };
        let mut w = field();
        w.set_stockpile(1, 20.0);
        w.take_notices();
        assert_eq!(w.apply(1, recon([90.0, 90.0])), Ok(()));
        let told = Notice::AirUnaffordable {
            player: 1,
            building: FIELD,
            job: AirJob::Recon,
            cost: 25.0,
        };
        assert_eq!(w.take_notices(), [told]);
        assert!(w.sorties.is_empty());
        // Too few ready: nothing, and nothing paid.
        let mut w = field();
        for berth in &mut w.buildings[FIELD].hangar[2..] {
            berth.health = 0.5;
        }
        assert_eq!(w.apply(1, recon([90.0, 90.0])), Err(Refused::Unable));
        assert_eq!(w.stockpile(1), 500.0);
        let mut w = field();
        w.apply(1, recon([90.0, 90.0])).unwrap();
        assert_eq!(w.stockpile(1), 475.0);
        assert_eq!(w.sorties.len(), 1);
        assert_eq!(w.sorties[0].template, Id::from("fighter"));
        until(&mut w, 30.0, |w| w.sorties[0].stage == Stage::Home);
        let u = w.sorties[0].plane.unwrap();
        assert!(distance(w.units[u].position, [90.0, 90.0]) < 1.0);
        until(&mut w, 60.0, |w| w.sorties[0].stage.over());
        assert!(matches!(w.sorties[0].stage, Stage::Landed(_)));
    }

    /// Moves the unit at `u` to `to` as a step would, and has the
    /// airfield look for intruders.
    fn step(w: &mut World, u: usize, to: [f32; 2]) {
        let unit = &mut w.units[u];
        (unit.previous, unit.position) = (unit.position, to);
        w.watch_for_intruders(FIELD);
    }

    #[test]
    fn an_airfield_notices_what_comes_into_its_guard_zone_and_forgets_it_out_of_range() {
        // `airfields#guard-range`, `airfields#guard-operational-range`
        let mut w = field();
        // What's there as the zone's laid comes in with it.
        let there = intruder(&mut w, [20.0, 70.0]);
        w.watch_for_intruders(FIELD);
        assert_eq!(w.buildings[FIELD].intruders, [Piece::Organization(there)]);
        let mut w = field();
        w.watch_for_intruders(FIELD);
        let r = intruder(&mut w, [20.0, 85.0]);
        let u = w.organizations[r].units[0];
        // Standing inside isn't coming in; stepping onto another tile is.
        let unit = &mut w.units[u];
        (unit.previous, unit.position) = ([20.0, 72.0], [20.0, 72.0]);
        w.watch_for_intruders(FIELD);
        assert!(w.buildings[FIELD].intruders.is_empty());
        step(&mut w, u, [20.0, 75.0]);
        assert_eq!(w.buildings[FIELD].intruders, [Piece::Organization(r)]);
        // Kept short of 48 from the airfield, forgotten at it.
        step(&mut w, u, [20.0, 97.5]);
        assert_eq!(w.buildings[FIELD].intruders, [Piece::Organization(r)]);
        step(&mut w, u, [20.0, 98.5]);
        assert!(w.buildings[FIELD].intruders.is_empty());
        // Beyond the zone, coming nearer isn't coming in.
        step(&mut w, u, [20.0, 90.0]);
        assert!(w.buildings[FIELD].intruders.is_empty());
    }

    #[test]
    fn every_ready_defender_goes_out_free_after_the_nearest_intruder_and_home_once_its_gone() {
        // `airfields#defensive-group`, `airfields#defender-targets`
        let mut w = field();
        let r = intruder(&mut w, [30.0, 70.0]);
        w.tick();
        assert!(w.stockpile(1) >= 500.0);
        let defenders: Vec<_> = (w.sorties.iter())
            .filter(|s| matches!(s.job, Job::Defend { .. }))
            .map(|s| s.template.as_str())
            .collect();
        assert_eq!(defenders, ["fighter", "fighter"]);
        let u = w.organizations[r].units[0];
        until(&mut w, 30.0, |w| w.units[u].health < 100.0);
        assert_eq!(
            w.sorties[0].job,
            Job::Defend {
                on: Some(Piece::Organization(r))
            }
        );
        w.units[u].die(w.ticks);
        run(&mut w, 0.5);
        assert!(w.sorties.iter().all(|s| s.stage == Stage::Home));
    }

    #[test]
    fn a_plane_flying_home_dies_at_once_when_its_airfield_is_destroyed() {
        // `airfields#airfield-destroyed`
        let mut w = field();
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        until(&mut w, 20.0, |w| w.sorties[0].stage == Stage::Out);
        w.sorties[0].mana = 0.0;
        w.tick();
        assert_eq!(w.sorties[0].stage, Stage::Home);
        let u = w.sorties[0].plane.unwrap();
        assert!(w.units[u].alive());
        w.buildings[FIELD].state = State::Destroyed(w.ticks);
        w.tick();
        assert!(!w.units[u].alive());
    }

    #[test]
    fn a_plane_comes_down_onto_the_runway_to_land() {
        // `airfields#landing-touchdown`
        let mut w = field();
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        run(&mut w, 3.0);
        w.sorties[0].mana = 0.0;
        let mut lowest = f32::MAX;
        while !w.sorties[0].stage.over() {
            w.tick();
            lowest = lowest.min(w.sorties[0].height);
        }
        assert!(lowest < 2.0, "{lowest}");
    }

    #[test]
    fn a_runway_passes_over_a_plane_that_dies_waiting_and_without_a_delay_launches_nothing() {
        // `airfields#runway-takeoff`
        let mut w = field();
        w.apply(1, on_house()).unwrap();
        w.apply(1, on_house()).unwrap();
        w.apply(1, escort(true)).unwrap();
        w.tick();
        let second = w.sorties[1].plane.unwrap();
        w.units[second].die(w.ticks);
        until(&mut w, 2.0, |w| w.sorties[2].stage == Stage::TakingOff);
        let delay = (1.0 / TICK).round() as u64;
        let took = |s: usize| w.sorties[s].took_off.unwrap();
        assert_eq!(took(2) - took(0), delay);
        let mut w = field();
        let airfield = w.rules.airfields.get_mut(&Id::from("airfield")).unwrap();
        airfield.runway.delay = 0.0;
        w.apply(1, on_house()).unwrap();
        run(&mut w, 5.0);
        assert_eq!(w.sorties[0].stage, Stage::Waiting);
    }

    #[test]
    fn a_plane_waiting_when_its_airfield_goes_dies_there() {
        // `airfields#airfield-destroyed`
        let mut w = field();
        w.apply(1, on_house()).unwrap();
        w.apply(1, on_house()).unwrap();
        w.tick();
        let u = w.sorties[1].plane.unwrap();
        w.buildings[FIELD].state = State::Destroyed(w.ticks);
        w.tick();
        assert!(!w.units[u].alive());
    }

    #[test]
    fn an_attack_gets_one_escort_even_once_its_shot_down() {
        // `airfields#escort-once`
        let mut w = field();
        w.apply(1, escort(true)).unwrap();
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        until(&mut w, 20.0, |w| {
            w.sorties.iter().all(|s| s.stage == Stage::Out)
        });
        for s in 1..3 {
            let u = w.sorties[s].plane.unwrap();
            w.units[u].die(w.ticks);
        }
        w.tick();
        for berth in &mut w.buildings[FIELD].hangar[2..] {
            berth.health = 1.0;
        }
        run(&mut w, 1.0);
        assert_eq!(w.sorties.len(), 3);
        // Switched off and on again, a new one goes.
        w.apply(1, escort(false)).unwrap();
        w.apply(1, escort(true)).unwrap();
        w.tick();
        let escorts = (w.sorties.iter())
            .filter(|s| s.wave == w.sorties[0].wave && matches!(s.job, Job::Escort { .. }))
            .map(|s| s.berth);
        assert_eq!(escorts.collect::<Vec<_>>(), [2, 3]);
        assert_eq!(w.sorties.len(), 5);
    }

    #[test]
    fn a_defender_out_of_shots_finishes_its_pass_and_comes_home() {
        // `airfields#defender-out-of-shots`
        let mut w = field();
        let r = intruder(&mut w, [30.0, 70.0]);
        let v = w.organizations[r].units[0];
        w.units[v].health = 1e6;
        w.tick();
        until(&mut w, 20.0, |w| w.sorties[0].stage == Stage::Out);
        until(&mut w, 20.0, |w| matches!(w.sorties[0].leg, Leg::Run(_)));
        w.sorties[0].mana = 0.0;
        w.tick();
        assert_eq!(w.sorties[0].stage, Stage::Out);
        until(&mut w, 30.0, |w| w.sorties[0].stage == Stage::Home);
        assert!(matches!(w.sorties[0].leg, Leg::Approach(_)));
        assert!(w.buildings[FIELD].intruders == [Piece::Organization(r)]);
    }

    #[test]
    fn defenders_of_an_airfield_packed_up_come_home_and_die_with_no_runway() {
        // `airfields#defenders-airfield-changes`, `airfields#airfield-packed`
        let mut w = field();
        intruder(&mut w, [30.0, 70.0]);
        w.tick();
        until(&mut w, 20.0, |w| w.sorties[0].stage == Stage::Out);
        let u = w.sorties[0].plane.unwrap();
        let truck = w
            .raise(&Id::from("truck"), 1, [20.0, 50.0], [0.0, 1.0])
            .unwrap();
        w.buildings[FIELD].state = State::Packed(w.ticks, truck);
        w.tick();
        assert!(w.buildings[FIELD].intruders.is_empty());
        assert!(!w.units[u].alive());
    }

    #[test]
    fn a_plane_comes_in_beyond_the_runway_lands_along_it_and_taxis_to_the_hangar() {
        // `airfields#landing-approach`, `airfields#landing-touchdown`
        let mut w = field();
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        until(&mut w, 20.0, |w| w.sorties[0].stage == Stage::Out);
        run(&mut w, 3.0);
        w.sorties[0].mana = 0.0;
        w.tick();
        let legs: Vec<_> = w.sorties[0].legs.iter().map(|p| (p.to, p.height)).collect();
        let ends = [
            ([39.2, 45.0], 17.0),
            ([24.8, 45.0], 1.0),
            ([20.0, 45.0], 1.0),
            ([17.5, 46.55], 1.0),
            ([17.5, 49.0], 1.0),
        ];
        assert_eq!(legs.len(), ends.len());
        for ((to, height), (want, h)) in legs.into_iter().zip(ends) {
            assert!(near(to, want) && height == h, "{to:?} {height}");
        }
        let u = w.sorties[0].plane.unwrap();
        until(&mut w, 60.0, |w| w.sorties[0].stage.over());
        assert!(near(w.units[u].position, [17.5, 49.0]));
        assert!(matches!(w.sorties[0].stage, Stage::Landed(_)));
    }

    #[test]
    fn a_plane_whose_airfield_is_gone_lands_at_another_of_its_sides_in_an_empty_place() {
        // `airfields#airfield-destroyed`, `airfields#landing-elsewhere`
        let mut w = field();
        let mut other = w.buildings[FIELD].clone();
        other.position = [20.0, 100.0];
        other.hangar = w.hangar(&Id::from("airfield"), true);
        other.hangar[1].health = 0.0;
        w.buildings.push(other);
        // Left empty for it.
        w.rules.resupply_rates.insert(Id::from("bomber"), 0.0);
        let b = w.buildings.len() - 1;
        w.apply(1, attack(None, [70.0, 80.0])).unwrap();
        until(&mut w, 20.0, |w| w.sorties[0].stage == Stage::Out);
        let u = w.sorties[0].plane.unwrap();
        w.units[u].health = 150.0;
        w.buildings[FIELD].state = State::Destroyed(w.ticks);
        w.sorties[0].mana = 0.0;
        w.tick();
        assert!(w.units[u].alive());
        assert_eq!(w.sorties[0].airfield, b);
        until(&mut w, 120.0, |w| w.sorties[0].stage.over());
        assert!(matches!(w.sorties[0].stage, Stage::Landed(_)));
        assert_eq!(w.buildings[b].hangar[1].health, 0.5);
    }

    #[test]
    fn defenders_that_cant_be_put_on_the_map_dont_hold_the_airfield_up() {
        // `airfields#defensive-group`
        let mut w = field();
        w.rules.raisable.remove(&Id::from("fighter"));
        intruder(&mut w, [30.0, 70.0]);
        w.tick();
        assert!(w.sorties.iter().all(|s| matches!(s.stage, Stage::Lost(_))));
    }
}
