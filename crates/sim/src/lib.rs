//! The rules of the game, played out with no rendering so that it can run
//! headless.
//!
//! A *match* is one play of a map: a *match setup* (the map, its players and
//! their nations, teams and rules) once it's running. Each *player* is a side
//! in it, controlled by a person or an AI.

pub mod path;

use std::collections::{HashMap, VecDeque};

use aaa_map::pathing::tile_at;
use aaa_map::{
    DeployPoints, Domain, Fighter, Id, Map, Obstacle, Packs, Pathing, Requires, Shape, Unpacks,
    Weapon,
};

pub use aaa_map::{Player, Relation, Team};

/// Seconds of play one [`World::tick`] advances.
pub const TICK: f32 = 0.1;

/// The share of its health a building taken over by force is left with.
pub const CAPTURE_HEALTH: f32 = 0.3;

/// How near a moving unit comes to an enemy to run into it.
pub const CONTACT: f32 = 1.5;

/// How near a packed building has to get to where it was sent to unpack.
pub const UNPACK_REACH: f32 = 1.5;

/// The share of a regiment's control range its pursued enemy has to come
/// within for it to engage.
pub const PURSUIT_REACH: f32 = 0.6;

/// How far, squared, the enemy a regiment pursues gets from where it's
/// headed, against how far, squared, it is from the regiment, before the
/// regiment turns after it: about 22.5° off course.
pub const PURSUIT_DRIFT: f32 = 0.14646;

/// A match in progress.
pub struct World {
    pub sides: Sides,
    pub pathing: Pathing,
    /// What each building template packs into, for those that can.
    pub packing: HashMap<String, Packs>,
    /// The buildings a player needs to commission a regiment with a member
    /// of each template, by template.
    pub requires: HashMap<Id, Requires>,
    pub regiments: Vec<Regiment>,
    pub units: Vec<Unit>,
    pub buildings: Vec<Building>,
    /// The obstacles that aren't buildings, such as trees and rocks.
    pub scenery: Vec<Obstacle>,
    /// Ticks played.
    pub ticks: u64,
    /// An id no object has yet.
    next_id: u32,
    /// An id no commission has yet.
    next_commission: u32,
}

pub struct Regiment {
    /// Its object's id in the map.
    pub id: u32,
    pub template: String,
    /// The owning [`Player::id`], the only one it takes orders from.
    pub owner: u32,
    /// Indices into [`World::units`].
    pub units: Vec<usize>,
    /// Its slowest member's, which they all keep to so as to stay together.
    pub speed: f32,
    /// Where it stands, or is headed: its members take their places in the
    /// formation around it.
    pub position: [f32; 2],
    /// The way it faces, or will once there.
    pub facing: [f32; 2],
    pub formations: Vec<aaa_map::Formation>,
    /// Index into `formations` of the one it's been put in.
    pub formation: usize,
    /// Index into `formations` of the one it fights in once engaged.
    pub combat: Option<usize>,
    /// How far from where it stands it takes on enemies that fire on it.
    pub operational_range: f32,
    /// Its zone of control's radius.
    pub control_range: f32,
    pub order: Order,
    /// The enemies it's engaged with. While there are any it holds its
    /// ground against them.
    pub engaged: Vec<Enemy>,
    /// Where it was headed and how it was to face there when it engaged,
    /// to carry on once it's done.
    resume: Option<([f32; 2], [f32; 2])>,
    /// For a packed building, such as a supply truck, what it unpacks into.
    pub unpacks: Option<Unpacks>,
    /// Where it's on its way to unpack.
    pub unpack_at: Option<[f32; 2]>,
    /// The building it unpacked into, by index into [`World::buildings`].
    pub unpacked_into: Option<usize>,
    /// For one just put out by a building, the building's rally point, to
    /// move on to once it's rolled out.
    pub rally: Option<[f32; 2]>,
}

impl Regiment {
    /// Index into `formations` of the one it's in: its combat formation
    /// while engaged or attacking.
    pub fn current_formation(&self) -> usize {
        let fighting =
            !self.engaged.is_empty() || matches!(self.order, Order::Attack | Order::Pursue(_));
        match fighting {
            true => self.combat.unwrap_or(self.formation),
            false => self.formation,
        }
    }

    /// Whether it takes on enemies that come within its control range or
    /// that it runs into, not only what fires on it: always while engaged
    /// or attacking, never while running, else as its formation is.
    pub fn aggressive(&self) -> bool {
        match self.order {
            Order::Run => false,
            Order::Attack | Order::Pursue(_) => true,
            Order::Move => !self.engaged.is_empty() || self.formations[self.formation].aggressive,
        }
    }
}

/// What a regiment can engage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enemy {
    /// Index into [`World::regiments`].
    Regiment(usize),
    /// Index into [`World::buildings`].
    Building(usize),
}

/// How a regiment goes where it's sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// Engaging enemies that fire on it or that it runs into on the way,
    /// and carrying on once they're dealt with. Standing is moving nowhere.
    #[default]
    Move,
    /// Ignoring enemies all the way there, then back to [`Order::Move`].
    Run,
    /// As [`Order::Move`], but in its combat formation and aggressive
    /// whatever formation it was put in, until it gets there; then back to
    /// [`Order::Move`].
    Attack,
    /// Going after the enemy regiment at this index into
    /// [`World::regiments`] wherever it goes, engaging it once near enough
    /// and engaging what it meets on the way as on [`Order::Move`], its
    /// members firing on that enemy first; until it's gone or can't be
    /// reached, then standing as on [`Order::Move`].
    Pursue(usize),
}

pub struct Unit {
    /// Its object's id in the map.
    pub id: u32,
    pub template: String,
    /// Index into [`World::regiments`].
    pub regiment: usize,
    pub domain: Domain,
    /// In world units.
    pub position: [f32; 2],
    /// Where it was a tick ago; see [`Unit::drawn_at`].
    pub previous: [f32; 2],
    /// The way it faces, a unit vector.
    pub facing: [f32; 2],
    /// The ground it takes up; `None` if it takes up none.
    pub shape: Option<Shape>,
    /// The way to face once it arrives.
    end_facing: [f32; 2],
    /// Waypoints still ahead, the next last.
    path: Vec<[f32; 2]>,
    /// The waypoint it set off for, and the way to it from where it set
    /// off: the course it steers for, and is there once it's come as far
    /// along.
    course: Option<([f32; 2], [f32; 2])>,
    /// World units a second it's going at.
    pub speed: f32,
    /// World units a second it gains each second; 0 for at once.
    acceleration: f32,
    /// Radians a second it turns; 0 for at once.
    turn_rate: f32,
    pub trailer: Option<Trailer>,
    /// Whether it's left its place to come within reach of an enemy.
    closing: bool,
    /// What it can take and deal; `None` for what can't be hurt.
    pub fighter: Option<Fighter>,
    pub health: f32,
    /// Seconds until each of its fighter's weapons can fire again.
    reloads: Vec<f32>,
    /// What it last fired on.
    pub target: Option<Target>,
    /// The tick it last fired on, and the index of the weapon it fired.
    pub shot: Option<(u64, usize)>,
    /// The tick it died on.
    pub died: Option<u64>,
    /// The tick it left the map, having unpacked into a building.
    pub unpacked: Option<u64>,
}

/// What a unit tows, dragged after the point it's coupled to so that it
/// trails round corners.
pub struct Trailer {
    pub template: String,
    pub position: [f32; 2],
    /// Where it was a tick ago, as [`Unit::previous`].
    pub previous: [f32; 2],
    /// The way it faces, a unit vector.
    pub facing: [f32; 2],
    /// Where it's coupled to its unit, from the unit: x to its right and y
    /// ahead.
    hitch: [f32; 2],
    /// The same point from the trailer.
    coupling: [f32; 2],
}

impl Trailer {
    fn new(trailer: &aaa_map::Trailer, position: [f32; 2], facing: [f32; 2]) -> Trailer {
        let (position, facing) = match trailer.placed {
            Some((_, at, way)) => (at, normalised(way).unwrap_or(facing)),
            None => {
                // Straight behind it.
                let hitch = aaa_map::from_local(position, facing, trailer.hitch);
                let [x, y] = trailer.coupling;
                (aaa_map::from_local(hitch, facing, [-x, -y]), facing)
            }
        };
        Trailer {
            template: trailer.template.clone(),
            position,
            previous: position,
            facing,
            hitch: trailer.hitch,
            coupling: trailer.coupling,
        }
    }

    /// Turns to point at its unit's hitch, and moves so that its coupling
    /// meets it.
    fn follow(&mut self, position: [f32; 2], facing: [f32; 2]) {
        let hitch = aaa_map::from_local(position, facing, self.hitch);
        let to = [hitch[0] - self.position[0], hitch[1] - self.position[1]];
        self.facing = normalised(to).unwrap_or(self.facing);
        let [x, y] = self.coupling;
        self.position = aaa_map::from_local(hitch, self.facing, [-x, -y]);
    }

    /// Where to draw it, as [`Unit::drawn_at`].
    pub fn drawn_at(&self, t: f32) -> [f32; 2] {
        let ([px, py], [x, y]) = (self.previous, self.position);
        [px + (x - px) * t, py + (y - py) * t]
    }
}

impl Unit {
    pub fn moving(&self) -> bool {
        !self.path.is_empty()
    }

    /// Whether it's still on the map and in one piece.
    pub fn alive(&self) -> bool {
        self.died.is_none() && self.unpacked.is_none()
    }

    fn die(&mut self, tick: u64) {
        self.health = 0.0;
        self.died = Some(tick);
        self.path.clear();
        self.speed = 0.0;
        self.target = None;
    }

    /// Plays out a tick of heading along its path at up to `top` speed:
    /// it speeds up to that, turns toward the way to its next waypoint as
    /// fast as it can and goes the way it faces, until it's come as far as
    /// that waypoint along that way.
    fn drive(&mut self, top: f32) {
        if self.path.is_empty() {
            self.speed = 0.0;
            return;
        }
        self.speed = match self.speed < top && self.acceleration > 0.0 {
            true => (self.speed + self.acceleration * TICK).min(top),
            false => top,
        };
        let mut left = self.speed * TICK;
        let mut turn = self.turn_rate * TICK;
        while let Some(&next) = self.path.last() {
            let course = match self.course {
                Some((to, course)) if to == next => course,
                _ => {
                    let way = [next[0] - self.position[0], next[1] - self.position[1]];
                    let course = normalised(way).unwrap_or(self.facing);
                    self.course = Some((next, course));
                    course
                }
            };
            match self.turn_rate > 0.0 {
                true => turn -= steer(&mut self.facing, course, turn),
                false => self.facing = course,
            }
            let (dx, dy) = (next[0] - self.position[0], next[1] - self.position[1]);
            let ahead = dx * course[0] + dy * course[1];
            if ahead > left {
                self.position[0] += self.facing[0] * left;
                self.position[1] += self.facing[1] * left;
                break;
            }
            match self.turn_rate > 0.0 {
                true => {
                    let ahead = ahead.max(0.0);
                    self.position[0] += self.facing[0] * ahead;
                    self.position[1] += self.facing[1] * ahead;
                    left -= ahead;
                }
                false => {
                    self.position = next;
                    left -= ahead.max(0.0);
                }
            }
            self.path.pop();
            self.course = None;
        }
        if self.path.is_empty() {
            self.speed = 0.0;
            self.facing = self.end_facing;
        }
    }

    /// Where to draw it a fraction `t` of the way through the tick that
    /// moved it from [`Unit::previous`] to [`Unit::position`].
    pub fn drawn_at(&self, t: f32) -> [f32; 2] {
        let ([px, py], [x, y]) = (self.previous, self.position);
        [px + (x - px) * t, py + (y - py) * t]
    }
}

/// An owned object that can be hurt but not move, such as a house, a
/// bunker or a city's centre.
pub struct Building {
    /// Its object's id in the map.
    pub id: u32,
    pub template: String,
    /// The owning [`Player::id`], which changes when it's captured.
    pub owner: u32,
    pub position: [f32; 2],
    /// The way its model's -y axis points, a unit vector.
    pub facing: [f32; 2],
    pub fighter: Fighter,
    pub health: f32,
    /// Seconds until each of its fighter's weapons can fire again.
    reloads: Vec<f32>,
    /// What it last fired on.
    pub target: Option<Target>,
    /// The tick it last fired on, and the index of the weapon it fired.
    pub shot: Option<(u64, usize)>,
    pub state: State,
    /// The ticks it began and finishes unpacking on, for one that
    /// unpacked from a truck during the match.
    pub unpacking: Option<(u64, u64)>,
    /// The ticks it began and finishes packing on.
    pub packing: Option<(u64, u64)>,
    /// What it's been asked to commission, the one under way first.
    pub queue: VecDeque<Commission>,
    /// Ticks spent on the first of the queue.
    pub progress: u64,
    /// Where what it commissions appears, then goes.
    pub deploy: Option<DeployPoints>,
    /// Where what it commissions moves on to once out, if set.
    pub rally: Option<[f32; 2]>,
    /// The tick it last changed hands on.
    pub captured: Option<u64>,
    /// For a city's centre, how far around it the city reaches.
    pub zone: Option<f32>,
    /// For a city's centre, indices into [`World::buildings`] of the
    /// buildings that change hands with it.
    pub members: Vec<usize>,
    /// The ground it keeps movers off until it's destroyed.
    pub obstacle: Option<Obstacle>,
}

/// How a building stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Standing,
    /// Out of health but still there, as buildings that can't be destroyed
    /// are left.
    Disabled,
    /// Out of health and gone, since the tick it fell on.
    Destroyed(u64),
    /// Packed up and gone, since the tick it packed on, into the regiment
    /// at this index into [`World::regiments`].
    Packed(u64, usize),
}

/// Something a building has been asked to commission.
#[derive(Debug, Clone)]
pub struct Commission {
    /// What it raises, standing anywhere: it's placed and given ids once
    /// it's done.
    pub regiment: aaa_map::Regiment,
    /// Ticks it takes.
    pub ticks: u64,
    /// Set once a building takes it on, to call it off by: unlike its
    /// place in the queue, it stays the same while orders are on their way.
    pub id: u32,
}

impl Commission {
    /// A commission taking `seconds`, at least a tick.
    pub fn new(regiment: aaa_map::Regiment, seconds: f32) -> Self {
        Commission {
            regiment,
            ticks: ((seconds / TICK).round() as u64).max(1),
            id: 0,
        }
    }
}

/// Why an order wasn't given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// It's someone else's, or gone.
    NotYours,
    /// It can't carry out that order.
    Unable,
    /// What it would put down doesn't fit there.
    NoRoom,
    /// Its owner lacks buildings that what it would raise needs.
    Lacking,
}

impl From<NotYours> for Refused {
    fn from(_: NotYours) -> Self {
        Refused::NotYours
    }
}

/// An order a player gives, as data, so that every copy of a match can
/// play the same orders out on the same tick. Regiments and buildings are
/// by index into [`World::regiments`] and [`World::buildings`], which only
/// ever grow, so the same in every copy.
#[derive(Debug, Clone)]
pub enum Command {
    /// Sends a regiment to `to`, turned to face the way it's going, each
    /// member making for its place in the formation there. An engaged
    /// regiment only sets off once it's done.
    Move { regiment: usize, to: [f32; 2] },
    /// As [`Command::Move`], but in its combat formation and taking on
    /// every enemy that comes within its control range on the way.
    Attack { regiment: usize, to: [f32; 2] },
    /// As [`Command::Move`], but breaking off any engagement and engaging
    /// nothing on the way.
    Run { regiment: usize, to: [f32; 2] },
    /// Sends a regiment after the enemy regiment `enemy`, breaking off any
    /// engagement, to engage it once near enough. Refused for a regiment
    /// with nothing to fire, or what isn't an enemy's.
    Pursue { regiment: usize, enemy: usize },
    /// Halts a regiment, its members forming up around where they stand on
    /// average, and drops where it was headed, so that once done with any
    /// engagement it stays put.
    Stop { regiment: usize },
    /// Sends a packed building to unpack at [`unpack_site`] for `at`.
    /// Refused where it can't unpack or the building wouldn't fit.
    Unpack { regiment: usize, at: [f32; 2] },
    /// Switches a regiment to another of its formations, its members
    /// taking their new places where it stands, or is headed.
    Formation { regiment: usize, formation: usize },
    /// Starts a building packing into what it packs into, such as a
    /// truck, dropping whatever it was asked to commission. Refused while
    /// it's unpacking or packing, or too hurt to pack.
    Pack { building: usize },
    /// Asks a building to commission something once it's done with what
    /// it was asked for before.
    Commission {
        building: usize,
        commission: Box<Commission>,
    },
    /// Calls off the commission with the [`Commission::id`] `commission`
    /// that a building was asked for, starting the next over if it was
    /// under way. Refused once it's gone.
    CancelCommission { building: usize, commission: u32 },
    /// Sets where what a building commissions moves on to once it's out,
    /// or, given `None`, has it stay where it comes out. Refused for a
    /// building that doesn't commission.
    Rally {
        building: usize,
        at: Option<[f32; 2]>,
    },
}

/// Where a packed building sent to unpack at `at` stands: the nearest
/// whole units to it.
pub fn unpack_site(at: [f32; 2]) -> [f32; 2] {
    at.map(f32::round)
}

impl Building {
    /// Whether it still takes part: it can be hit and fire.
    pub fn standing(&self) -> bool {
        self.state == State::Standing
    }

    /// Whether it's left the map, destroyed or packed up.
    pub fn gone(&self) -> bool {
        matches!(self.state, State::Destroyed(_) | State::Packed(..))
    }

    /// Whether it's unpacking or packing, and so can't take orders.
    pub fn busy(&self) -> bool {
        self.unpacking.is_some() || self.packing.is_some()
    }

    /// How far a point is from it, to the edge of the ground it covers.
    fn distance(&self, from: [f32; 2]) -> f32 {
        match &self.obstacle {
            Some(o) => o.distance(from),
            None => distance(from, self.position),
        }
    }
}

/// Something a weapon can fire on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Index into [`World::units`].
    Unit(usize),
    /// Index into [`World::buildings`].
    Building(usize),
}

impl World {
    pub fn new(map: &Map) -> World {
        let sides = Sides {
            players: map.players.clone(),
            teams: map.teams.clone(),
        };
        let mut world = World::from_regiments(sides, map.pathing.clone(), &map.regiments);
        world.add_buildings(&map.buildings);
        world.scenery = map.scenery.clone();
        let members = map
            .things
            .iter()
            .flat_map(|t| t.organization())
            .flat_map(|o| &o.members);
        let ids = map.things.iter().chain(members).map(|t| t.id);
        world.next_id = world.next_id.max(ids.max().map_or(1, |id| id + 1));
        world
    }

    pub fn from_regiments(
        sides: Sides,
        pathing: Pathing,
        regiments: &[aaa_map::Regiment],
    ) -> World {
        let mut world = World {
            sides,
            pathing,
            regiments: Vec::new(),
            units: Vec::new(),
            buildings: Vec::new(),
            scenery: Vec::new(),
            ticks: 0,
            next_id: 1,
            next_commission: 0,
            packing: HashMap::new(),
            requires: HashMap::new(),
        };
        for regiment in regiments {
            world.add_regiment(regiment);
        }
        world
    }

    /// Adds a regiment as the map has it, standing where its members do,
    /// returning its index.
    pub fn add_regiment(&mut self, regiment: &aaa_map::Regiment) -> usize {
        let r = self.regiments.len();
        let first = self.units.len();
        for m in &regiment.members {
            let [x, y, _] = m.thing.position;
            let facing = normalised([m.thing.facing[0], m.thing.facing[1]]).unwrap_or([0.0, -1.0]);
            let (tx, ty) = tile_at([x, y]);
            // Units start on what they travel on.
            let domain = match self.pathing.passable(tx, ty, Domain::Land)
                || !self.pathing.passable(tx, ty, Domain::Naval)
            {
                true => Domain::Land,
                false => Domain::Naval,
            };
            let fighter = m.fighter.clone();
            self.units.push(Unit {
                health: fighter.as_ref().map_or(0.0, |f| f.health),
                reloads: fighter
                    .as_ref()
                    .map_or(Vec::new(), |f| vec![0.0; f.weapons.len()]),
                fighter,
                target: None,
                shot: None,
                died: None,
                unpacked: None,
                id: m.thing.id,
                template: m.thing.template.clone(),
                regiment: r,
                domain,
                position: [x, y],
                previous: [x, y],
                facing,
                shape: m.shape,
                end_facing: facing,
                path: Vec::new(),
                course: None,
                speed: 0.0,
                acceleration: m.acceleration,
                turn_rate: m.turn_rate,
                trailer: m.trailer.as_ref().map(|t| Trailer::new(t, [x, y], facing)),
                closing: false,
            });
        }
        let units: Vec<usize> = (first..self.units.len()).collect();
        let position = match regiment.primary {
            Some(p) => self.units[first + p].position,
            None => self.average(&units),
        };
        self.regiments.push(Regiment {
            id: regiment.id,
            template: regiment.template.clone(),
            owner: regiment.owner,
            units,
            speed: regiment
                .members
                .iter()
                .map(|m| m.speed)
                .fold(f32::INFINITY, f32::min),
            position,
            facing: normalised(regiment.facing).unwrap_or([0.0, -1.0]),
            formations: regiment.formations.clone(),
            formation: regiment.formation,
            combat: regiment.combat,
            operational_range: regiment.operational_range.unwrap_or(0.0),
            control_range: regiment.control_range.unwrap_or(0.0),
            order: Order::Move,
            engaged: Vec::new(),
            resume: None,
            unpacks: regiment.unpacks.clone(),
            unpack_at: None,
            unpacked_into: None,
            rally: None,
        });
        let ids = regiment.members.iter().map(|m| m.thing.id);
        self.next_id = ids
            .chain([regiment.id])
            .fold(self.next_id, |n, id| n.max(id + 1));
        r
    }

    /// Takes `n` ids no object has, returning the first.
    pub fn take_ids(&mut self, n: u32) -> u32 {
        let first = self.next_id;
        self.next_id += n;
        first
    }

    /// Adds buildings as the map has them, those out of health already
    /// disabled. Their ground is taken to be blocked already.
    pub fn add_buildings(&mut self, buildings: &[aaa_map::Building]) {
        let first = self.buildings.len();
        for b in buildings {
            let health = b.fighter.health * b.health;
            self.buildings.push(Building {
                id: b.id,
                template: b.template.clone(),
                owner: b.owner,
                position: b.position,
                facing: b.facing,
                reloads: vec![0.0; b.fighter.weapons.len()],
                fighter: b.fighter.clone(),
                health,
                target: None,
                shot: None,
                state: match health > 0.0 {
                    true => State::Standing,
                    false => State::Disabled,
                },
                captured: None,
                zone: b.zone,
                members: Vec::new(),
                obstacle: b.obstacle.clone(),
                unpacking: None,
                packing: None,
                queue: VecDeque::new(),
                progress: 0,
                deploy: b.deploy,
                rally: None,
            });
            self.next_id = self.next_id.max(b.id + 1);
        }
        for (i, b) in buildings.iter().enumerate() {
            let index =
                |id: &u32| (first..self.buildings.len()).find(|&j| self.buildings[j].id == *id);
            self.buildings[first + i].members = b.members.iter().filter_map(index).collect();
        }
    }

    /// Where a regiment's surviving members stand on average.
    pub fn centre(&self, regiment: usize) -> [f32; 2] {
        let survivors: Vec<usize> = self.survivors(regiment).collect();
        self.average(&survivors)
    }

    /// A regiment's members still alive.
    pub fn survivors(&self, regiment: usize) -> impl Iterator<Item = usize> + '_ {
        let units = &self.regiments[regiment].units;
        units.iter().copied().filter(|&u| self.units[u].alive())
    }

    /// Where a regiment is headed: where it stands once there, or, while
    /// fighting on the way, where it carries on to after.
    pub fn destination(&self, regiment: usize) -> [f32; 2] {
        let r = &self.regiments[regiment];
        r.resume.map_or(r.position, |(to, _)| to)
    }

    /// Whether a regiment is still on its way somewhere: moving, or
    /// fighting before it carries on.
    pub fn underway(&self, regiment: usize) -> bool {
        let r = &self.regiments[regiment];
        r.resume.is_some() || self.survivors(regiment).any(|u| self.units[u].moving())
    }

    /// Whether a regiment has lost every member.
    pub fn destroyed(&self, regiment: usize) -> bool {
        self.survivors(regiment).next().is_none()
    }

    fn average(&self, units: &[usize]) -> [f32; 2] {
        let n = units.len().max(1) as f32;
        let [x, y] = units.iter().fold([0.0, 0.0], |[x, y], &u| {
            let [ux, uy] = self.units[u].position;
            [x + ux, y + uy]
        });
        [x / n, y / n]
    }

    /// Kills a regiment's surviving members outright.
    pub fn destroy(&mut self, regiment: usize) {
        for u in self.regiments[regiment].units.clone() {
            let unit = &mut self.units[u];
            if unit.alive() {
                unit.die(self.ticks);
            }
        }
    }

    /// Whether `player` may give `regiment` orders.
    pub fn commands(&self, player: u32, regiment: usize) -> bool {
        self.regiments
            .get(regiment)
            .is_some_and(|r| r.owner == player)
            && !self.destroyed(regiment)
    }

    /// Whether `player` may give `command` now: what [`World::apply`]
    /// would refuse it for, without carrying it out.
    pub fn check(&self, player: u32, command: &Command) -> Result<(), Refused> {
        match *command {
            Command::Move { regiment, .. }
            | Command::Attack { regiment, .. }
            | Command::Run { regiment, .. }
            | Command::Stop { regiment }
            | Command::Formation { regiment, .. } => Ok(self.may_command(player, regiment)?),
            Command::Pursue { regiment, enemy } => self.may_pursue(player, regiment, enemy),
            Command::Unpack { regiment, at } => self.may_unpack(player, regiment, at).map(drop),
            Command::Pack { building } => self.may_pack(player, building).map(drop),
            Command::Commission {
                building,
                ref commission,
            } => self.may_commission(player, building, &commission.regiment),
            Command::CancelCommission {
                building,
                commission,
            } => self.may_cancel(player, building, commission).map(drop),
            Command::Rally { building, .. } => self.may_rally(player, building),
        }
    }

    /// Carries out `command` from `player`, unless it's refused.
    pub fn apply(&mut self, player: u32, command: Command) -> Result<(), Refused> {
        match command {
            Command::Move { regiment, to } => self.order_move(player, regiment, to)?,
            Command::Attack { regiment, to } => self.order_attack(player, regiment, to)?,
            Command::Run { regiment, to } => self.order_run(player, regiment, to)?,
            Command::Pursue { regiment, enemy } => self.order_pursue(player, regiment, enemy)?,
            Command::Stop { regiment } => self.order_stop(player, regiment)?,
            Command::Unpack { regiment, at } => {
                self.order_unpack(player, regiment, at)?;
            }
            Command::Formation {
                regiment,
                formation,
            } => self.set_formation(player, regiment, formation)?,
            Command::Pack { building } => self.order_pack(player, building)?,
            Command::Commission {
                building,
                commission,
            } => self.commission(player, building, *commission)?,
            Command::CancelCommission {
                building,
                commission,
            } => self.cancel_commission(player, building, commission)?,
            Command::Rally { building, at } => self.set_rally(player, building, at)?,
        }
        Ok(())
    }

    fn may_command(&self, player: u32, regiment: usize) -> Result<(), NotYours> {
        match self.commands(player, regiment) {
            true => Ok(()),
            false => Err(NotYours),
        }
    }

    /// The building, if `player`'s and standing.
    fn standing_for(&self, player: u32, building: usize) -> Result<&Building, Refused> {
        self.buildings
            .get(building)
            .filter(|b| b.owner == player && b.standing())
            .ok_or(Refused::NotYours)
    }

    fn may_pursue(&self, player: u32, regiment: usize, enemy: usize) -> Result<(), Refused> {
        self.may_command(player, regiment)?;
        let armed = self.survivors(regiment).any(|u| {
            self.units[u]
                .fighter
                .as_ref()
                .is_some_and(|f| !f.weapons.is_empty())
        });
        if !armed
            || enemy >= self.regiments.len()
            || self.destroyed(enemy)
            || !self.hostile(regiment, Enemy::Regiment(enemy))
        {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    /// Where the regiment stops to unpack, relative to where it unpacks,
    /// if it can there.
    fn may_unpack(&self, player: u32, regiment: usize, at: [f32; 2]) -> Result<[f32; 2], Refused> {
        self.may_command(player, regiment)?;
        let Some(unpacks) = &self.regiments[regiment].unpacks else {
            return Err(Refused::Unable);
        };
        if !self.room_to_unpack(unpacks, unpack_site(at)) {
            return Err(Refused::NoRoom);
        }
        Ok(unpacks.offset)
    }

    /// How long the building takes to pack, if it can now.
    fn may_pack(&self, player: u32, building: usize) -> Result<u64, Refused> {
        let b = self.standing_for(player, building)?;
        let Some(packs) = self.packing.get(&b.template) else {
            return Err(Refused::Unable);
        };
        if b.busy() || b.health <= packs.health_min * b.fighter.health {
            return Err(Refused::Unable);
        }
        Ok((packs.seconds / TICK).round() as u64)
    }

    fn may_commission(
        &self,
        player: u32,
        building: usize,
        regiment: &aaa_map::Regiment,
    ) -> Result<(), Refused> {
        let b = self.standing_for(player, building)?;
        if b.deploy.is_none() || b.busy() {
            return Err(Refused::Unable);
        }
        if !self.lacks(player, regiment).is_empty() {
            return Err(Refused::Lacking);
        }
        Ok(())
    }

    /// The buildings `player` would need to commission `regiment` but
    /// hasn't standing, by their current ids.
    pub fn lacks(&self, player: u32, regiment: &aaa_map::Regiment) -> Vec<&Id> {
        // Only buildings done unpacking count, and only the player's own,
        // not its allies'.
        let has = |ids: &[Id]| {
            self.buildings.iter().any(|b| {
                b.owner == player
                    && b.standing()
                    && !b.busy()
                    && ids
                        .iter()
                        .any(|id| id.as_str().eq_ignore_ascii_case(&b.template))
            })
        };
        let mut out: Vec<&Id> = Vec::new();
        for m in &regiment.members {
            let Some(r) = self.requires.get(&Id::from(m.thing.template.as_str())) else {
                continue;
            };
            let missing: Vec<&Id> = r
                .structures
                .iter()
                .filter(|ids| !has(ids))
                .map(|ids| &ids[0])
                .collect();
            if r.any && missing.len() < r.structures.len() {
                continue;
            }
            for id in missing {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
        out
    }

    fn may_rally(&self, player: u32, building: usize) -> Result<(), Refused> {
        if self.standing_for(player, building)?.deploy.is_none() {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    /// Where the commission is in the building's queue, if it's there.
    fn may_cancel(&self, player: u32, building: usize, commission: u32) -> Result<usize, Refused> {
        let b = self
            .buildings
            .get(building)
            .filter(|b| b.owner == player)
            .ok_or(Refused::NotYours)?;
        b.queue
            .iter()
            .position(|c| c.id == commission)
            .ok_or(Refused::Unable)
    }

    fn order_move(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.advance(player, regiment, target, Order::Move)
    }

    fn order_attack(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.advance(player, regiment, target, Order::Attack)
    }

    fn order_pursue(&mut self, player: u32, regiment: usize, enemy: usize) -> Result<(), Refused> {
        self.may_pursue(player, regiment, enemy)?;
        let r = &mut self.regiments[regiment];
        r.order = Order::Pursue(enemy);
        r.unpack_at = None;
        r.rally = None;
        r.engaged.clear();
        r.resume = None;
        self.chase(regiment, enemy, true);
        Ok(())
    }

    fn advance(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
        order: Order,
    ) -> Result<(), NotYours> {
        self.may_command(player, regiment)?;
        let r = &mut self.regiments[regiment];
        r.order = order;
        r.unpack_at = None;
        r.rally = None;
        let from = r.position;
        let facing = normalised([target[0] - from[0], target[1] - from[1]]).unwrap_or(r.facing);
        match r.engaged.is_empty() {
            true => self.send(regiment, target, facing),
            false => r.resume = Some((target, facing)),
        }
        Ok(())
    }

    fn order_run(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.may_command(player, regiment)?;
        let r = &mut self.regiments[regiment];
        r.order = Order::Run;
        r.unpack_at = None;
        r.rally = None;
        r.engaged.clear();
        r.resume = None;
        let from = r.position;
        let facing = normalised([target[0] - from[0], target[1] - from[1]]).unwrap_or(r.facing);
        self.send(regiment, target, facing);
        Ok(())
    }

    fn order_stop(&mut self, player: u32, regiment: usize) -> Result<(), NotYours> {
        self.may_command(player, regiment)?;
        let here = self.centre(regiment);
        let r = &mut self.regiments[regiment];
        r.order = Order::Move;
        r.resume = None;
        r.unpack_at = None;
        r.rally = None;
        if r.engaged.is_empty() {
            let facing = r.facing;
            self.send(regiment, here, facing);
        }
        Ok(())
    }

    fn order_unpack(
        &mut self,
        player: u32,
        regiment: usize,
        at: [f32; 2],
    ) -> Result<[f32; 2], Refused> {
        let offset = self.may_unpack(player, regiment, at)?;
        let at = unpack_site(at);
        let stop = [at[0] + offset[0], at[1] + offset[1]];
        self.order_move(player, regiment, stop)?;
        self.regiments[regiment].unpack_at = Some(at);
        Ok(at)
    }

    /// Whether what `unpacks` makes would fit standing at `at`, with the
    /// room around it buildings need.
    pub fn room_to_unpack(&self, unpacks: &Unpacks, at: [f32; 2]) -> bool {
        match (&unpacks.building.obstacle, unpacks.room) {
            (Some(o), Some(shape)) => {
                let placed = Obstacle {
                    position: at,
                    facing: o.facing,
                    shape,
                };
                self.pathing.room_for(&placed, Domain::Land)
            }
            _ => {
                let (x, y) = tile_at(at);
                self.pathing.passable(x, y, Domain::Land)
            }
        }
    }

    fn order_pack(&mut self, player: u32, building: usize) -> Result<(), Refused> {
        let ticks = self.may_pack(player, building)?;
        let b = &mut self.buildings[building];
        b.queue.clear();
        b.progress = 0;
        b.packing = Some((self.ticks, self.ticks + ticks));
        Ok(())
    }

    fn commission(
        &mut self,
        player: u32,
        building: usize,
        mut commission: Commission,
    ) -> Result<(), Refused> {
        self.may_commission(player, building, &commission.regiment)?;
        commission.id = self.next_commission;
        self.next_commission += 1;
        self.buildings[building].queue.push_back(commission);
        Ok(())
    }

    fn set_rally(
        &mut self,
        player: u32,
        building: usize,
        at: Option<[f32; 2]>,
    ) -> Result<(), Refused> {
        self.may_rally(player, building)?;
        self.buildings[building].rally = at;
        Ok(())
    }

    fn cancel_commission(
        &mut self,
        player: u32,
        building: usize,
        commission: u32,
    ) -> Result<(), Refused> {
        let index = self.may_cancel(player, building, commission)?;
        let b = &mut self.buildings[building];
        b.queue.remove(index);
        if index == 0 {
            b.progress = 0;
        }
        Ok(())
    }

    fn send(&mut self, regiment: usize, target: [f32; 2], facing: [f32; 2]) {
        let r = &mut self.regiments[regiment];
        r.position = target;
        r.facing = facing;
        self.take_places(regiment);
    }

    fn set_formation(
        &mut self,
        player: u32,
        regiment: usize,
        formation: usize,
    ) -> Result<(), NotYours> {
        self.may_command(player, regiment)?;
        let r = &mut self.regiments[regiment];
        if formation < r.formations.len() && formation != r.formation {
            r.formation = formation;
            // An engaged regiment keeps to its combat formation for now.
            if r.current_formation() == formation {
                self.take_places(regiment);
            }
        }
        Ok(())
    }

    /// Sends each of a regiment's members to its place in the formation.
    fn take_places(&mut self, regiment: usize) {
        let r = &self.regiments[regiment];
        let facing = r.facing;
        let offsets = &r.formations[r.current_formation()].offsets;
        let places: Vec<(usize, [f32; 2])> = r
            .units
            .iter()
            .zip(offsets)
            .map(|(&u, &offset)| (u, aaa_map::from_local(r.position, facing, offset)))
            .collect();
        for (u, place) in places {
            if !self.units[u].alive() {
                continue;
            }
            let way = self.way(u, place);
            let unit = &mut self.units[u];
            unit.path = way;
            unit.closing = false;
            unit.end_facing = facing;
            if unit.path.is_empty() {
                unit.facing = facing;
            }
        }
    }

    /// A unit's way to `to`, last waypoint first. It passes through the
    /// obstacles it's caught on, as one put out through its building, or
    /// beside one unpacked next to it, is: those its shape overlaps, and
    /// those blocking its tile, which reach further than their shapes do.
    fn way(&self, u: usize, to: [f32; 2]) -> Vec<[f32; 2]> {
        let unit = &self.units[u];
        let tile = tile_at(unit.position);
        let body = unit.shape.map(|shape| Obstacle {
            position: unit.position,
            facing: unit.facing,
            shape,
        });
        let caught: Vec<&Obstacle> = self
            .buildings
            .iter()
            .filter(|b| !b.gone())
            .filter_map(|b| b.obstacle.as_ref())
            .chain(&self.scenery)
            .filter(|o| o.covers(tile) || body.as_ref().is_some_and(|b| b.overlaps(o)))
            .collect();
        let mut way = if caught.is_empty() {
            path::find(&self.pathing, unit.domain, unit.position, to)
        } else {
            let pathing = self.pathing.without(caught);
            path::find(&pathing, unit.domain, unit.position, to)
        };
        way.reverse();
        way
    }

    /// Plays one [`TICK`].
    pub fn tick(&mut self) {
        self.travel();
        let fired_on = self.fight();
        self.engage(&fired_on);
        self.close_in();
        self.occupy();
        self.produce();
        self.rally();
        self.unpack();
        self.ticks += 1;
    }

    /// Moves buildings on with what they're commissioning, and puts out
    /// what's done where they deploy it, sent on to where it goes.
    fn produce(&mut self) {
        for b in 0..self.buildings.len() {
            let building = &mut self.buildings[b];
            if !building.standing() || building.busy() {
                continue;
            }
            let (Some(first), Some(deploy)) = (building.queue.front(), building.deploy) else {
                continue;
            };
            building.progress += 1;
            if building.progress < first.ticks {
                continue;
            }
            building.progress = 0;
            let Some(done) = building.queue.pop_front() else {
                continue;
            };
            let (position, facing, owner) = (building.position, building.facing, building.owner);
            let rally = building.rally;
            let from = deploy_point(position, facing, deploy.creation);
            let to = deploy_point(position, facing, deploy.deployment);
            let heading = normalised([to[0] - from[0], to[1] - from[1]]).unwrap_or(facing);
            let mut regiment = done.regiment;
            regiment.owner = owner;
            let ids = regiment.members.len() as u32 + u32::from(!regiment.lone());
            regiment.renumber(self.take_ids(ids));
            regiment.place(from, heading);
            let r = self.add_regiment(&regiment);
            self.roll_out(r, to, heading);
            self.regiments[r].rally = rally;
        }
    }

    /// Moves regiments that have rolled out of what put them out on to its
    /// rally point.
    fn rally(&mut self) {
        for r in 0..self.regiments.len() {
            let Some(at) = self.regiments[r].rally else {
                continue;
            };
            if self.destroyed(r) {
                self.regiments[r].rally = None;
            } else if !self.underway(r) {
                let owner = self.regiments[r].owner;
                let _ = self.order_move(owner, r, at);
            }
        }
    }

    /// Sends a regiment just put out straight to `to`, through whatever
    /// put it out, each member to its place in the formation there.
    fn roll_out(&mut self, regiment: usize, to: [f32; 2], facing: [f32; 2]) {
        let r = &mut self.regiments[regiment];
        r.position = to;
        r.facing = facing;
        let offsets = &r.formations[r.current_formation()].offsets;
        for (&u, &offset) in r.units.iter().zip(offsets) {
            let unit = &mut self.units[u];
            unit.path = vec![aaa_map::from_local(to, facing, offset)];
            unit.end_facing = facing;
        }
    }

    /// Unpacks the packed buildings that have reached where they were sent
    /// to unpack, each leaving the map for the building it makes, and
    /// calls it off for those that can't get there or no longer have room.
    fn unpack(&mut self) {
        for r in 0..self.regiments.len() {
            let Some(at) = self.regiments[r].unpack_at else {
                continue;
            };
            if self.destroyed(r) {
                self.regiments[r].unpack_at = None;
                continue;
            }
            if self.underway(r) {
                continue;
            }
            self.regiments[r].unpack_at = None;
            let Some(unpacks) = self.regiments[r].unpacks.clone() else {
                continue;
            };
            let stop = [at[0] + unpacks.offset[0], at[1] + unpacks.offset[1]];
            if !within(self.centre(r), stop, UNPACK_REACH) || !self.room_to_unpack(&unpacks, at) {
                continue;
            }
            // It keeps the share of its health it had as a truck.
            let health = self
                .survivors(r)
                .filter_map(|u| Some(self.units[u].health / self.units[u].fighter.as_ref()?.health))
                .fold(1.0_f32, f32::min);
            for u in self.regiments[r].units.clone() {
                let unit = &mut self.units[u];
                if unit.alive() {
                    unit.unpacked = Some(self.ticks);
                    unit.path.clear();
                    unit.target = None;
                }
            }
            let mut building = unpacks.building;
            building.id = self.take_ids(1);
            building.owner = self.regiments[r].owner;
            building.position = at;
            building.health = health;
            if let Some(o) = &mut building.obstacle {
                o.position = at;
                self.pathing.block(o);
            }
            let b = self.buildings.len();
            self.regiments[r].unpacked_into = Some(b);
            self.add_buildings(std::slice::from_ref(&building));
            let ticks = (unpacks.seconds / TICK).round() as u64;
            self.buildings[b].unpacking = Some((self.ticks, self.ticks + ticks));
        }
        for b in &mut self.buildings {
            if let Some((_, end)) = b.unpacking
                && self.ticks >= end
            {
                b.unpacking = None;
            }
        }
        for b in 0..self.buildings.len() {
            if let Some((_, end)) = self.buildings[b].packing
                && self.ticks >= end
            {
                self.pack_up(b);
            }
        }
    }

    /// Takes a building that's done packing off the map, putting out what
    /// it packs into where it stood, with the share of health it had.
    fn pack_up(&mut self, b: usize) {
        let building = &mut self.buildings[b];
        building.packing = None;
        if !building.standing() {
            return;
        }
        let Some(packs) = self.packing.get(&building.template).cloned() else {
            return;
        };
        let share = building.health / building.fighter.health;
        let at = building.position;
        let owner = building.owner;
        if let Some(o) = building.obstacle.clone() {
            self.pathing.unblock(&o);
        }
        let mut truck = packs.truck;
        truck.owner = owner;
        let ids = truck.members.len() as u32 + u32::from(!truck.lone());
        truck.renumber(self.take_ids(ids));
        truck.place(
            [at[0] + packs.offset[0], at[1] + packs.offset[1]],
            packs.facing,
        );
        let r = self.add_regiment(&truck);
        for u in self.regiments[r].units.clone() {
            let unit = &mut self.units[u];
            if let Some(f) = &unit.fighter {
                unit.health = f.health * share;
            }
        }
        self.buildings[b].state = State::Packed(self.ticks, r);
    }

    /// Ends the engagements whose enemies are gone, then engages regiments
    /// with what `fired_on` says fired on them, by regiment, where it's in
    /// their operational range, [`Regiment::aggressive`] ones with enemies
    /// that come within their control range and enemy units their moving
    /// units run into, and those on [`Order::Pursue`] with their enemy
    /// once near enough. Regiments on [`Order::Run`] engage nothing.
    fn engage(&mut self, fired_on: &[(usize, Target)]) {
        for r in 0..self.regiments.len() {
            if let Order::Pursue(e) = self.regiments[r].order
                && self.lost(r, e)
            {
                self.give_up(r);
            }
            let regiment = &self.regiments[r];
            if !matches!(regiment.order, Order::Move | Order::Pursue(_))
                && regiment.engaged.is_empty()
                && !self.survivors(r).any(|u| self.units[u].moving())
            {
                let before = regiment.current_formation();
                self.regiments[r].order = Order::Move;
                if self.regiments[r].current_formation() != before {
                    self.take_places(r);
                }
            }
            let engaged = std::mem::take(&mut self.regiments[r].engaged);
            if engaged.is_empty() {
                continue;
            }
            let left: Vec<Enemy> = engaged
                .into_iter()
                .filter(|&e| self.within_range(r, e))
                .collect();
            let done = left.is_empty();
            self.regiments[r].engaged = left;
            if done {
                self.disengage(r);
            }
        }
        let mut meetings: Vec<(usize, Enemy)> = Vec::new();
        for &(r, from) in fired_on {
            let reach = self.regiments[r].operational_range;
            if self.reach(self.centre(r), from) <= reach {
                let enemy = match from {
                    Target::Unit(u) => Enemy::Regiment(self.units[u].regiment),
                    Target::Building(b) => Enemy::Building(b),
                };
                meetings.push((r, enemy));
            }
        }
        for (u, unit) in self.units.iter().enumerate() {
            if !unit.alive() || !unit.moving() || !self.regiments[unit.regiment].aggressive() {
                continue;
            }
            let met = self.units.iter().enumerate().find(|&(v, other)| {
                v != u
                    && other.alive()
                    && within(unit.position, other.position, CONTACT)
                    && self.hostile(unit.regiment, Enemy::Regiment(other.regiment))
            });
            if let Some((_, other)) = met {
                meetings.push((unit.regiment, Enemy::Regiment(other.regiment)));
            }
        }
        for r in 0..self.regiments.len() {
            if self.regiments[r].aggressive() && !self.destroyed(r) {
                meetings.extend(self.in_control(r).into_iter().map(|e| (r, e)));
            }
        }
        for (r, enemy) in meetings {
            let regiment = &self.regiments[r];
            if regiment.order == Order::Run
                || regiment.engaged.contains(&enemy)
                || !self.hostile(r, enemy)
                || self.destroyed(r)
            {
                continue;
            }
            let first = regiment.engaged.is_empty();
            self.regiments[r].engaged.push(enemy);
            if first {
                self.hold(r, enemy);
            }
        }
        for r in 0..self.regiments.len() {
            if let Order::Pursue(e) = self.regiments[r].order
                && self.regiments[r].engaged.is_empty()
            {
                self.pursue(r, e);
            }
        }
    }

    /// Sends each armed member of an engaged regiment that can fire on
    /// nothing after the enemy it would fire on first, and stops those
    /// that have come within reach to fire.
    fn close_in(&mut self) {
        for u in 0..self.units.len() {
            let unit = &self.units[u];
            let r = unit.regiment;
            if !unit.alive() || self.regiments[r].engaged.is_empty() {
                continue;
            }
            let Some(fighter) = unit.fighter.as_ref() else {
                continue;
            };
            let s = Target::Unit(u);
            let weapons = &fighter.weapons;
            if weapons
                .iter()
                .any(|w| self.targets().any(|t| self.can_hit(s, w, t)))
            {
                if unit.closing {
                    let unit = &mut self.units[u];
                    unit.path.clear();
                    unit.closing = false;
                }
                continue;
            }
            // Those still taking their places carry on.
            if unit.moving() && !unit.closing {
                continue;
            }
            let Some((goal, range)) = self.quarry(u) else {
                continue;
            };
            if unit.closing
                && unit
                    .path
                    .first()
                    .is_some_and(|&end| self.reach(end, goal) <= range)
            {
                continue;
            }
            let way = self.way(u, self.position(goal));
            let unit = &mut self.units[u];
            unit.path = way;
            unit.closing = true;
        }
    }

    /// What a member of an engaged regiment goes after when it can fire on
    /// nothing, with the reach of its longest weapon that fires on it: the
    /// nearest of the enemy its regiment pursues, where that's within its
    /// operational range, else the nearest of what it's engaged with.
    fn quarry(&self, u: usize) -> Option<(Target, f32)> {
        let r = self.units[u].regiment;
        let regiment = &self.regiments[r];
        let s = Target::Unit(u);
        let from = self.units[u].position;
        let weapons = &self.units[u].fighter.as_ref()?.weapons;
        let range = |t: Target| {
            let ranges = weapons.iter().filter(|w| self.can_target(s, w, t));
            ranges.map(|w| w.range).max_by(f32::total_cmp)
        };
        let nearest = |ts: Vec<Target>| {
            ts.into_iter()
                .filter_map(|t| Some((t, range(t)?)))
                .min_by(|a, b| self.reach(from, a.0).total_cmp(&self.reach(from, b.0)))
        };
        let units = |e: usize| self.survivors(e).map(Target::Unit).collect::<Vec<_>>();
        if let Order::Pursue(e) = regiment.order {
            let here = self.centre(r);
            let pursued = units(e);
            let near = pursued
                .iter()
                .any(|&t| self.reach(here, t) <= regiment.operational_range);
            if let Some(found) = nearest(pursued).filter(|_| near) {
                return Some(found);
            }
        }
        let engaged = regiment.engaged.iter().flat_map(|&enemy| match enemy {
            Enemy::Regiment(e) => units(e),
            Enemy::Building(b) => vec![Target::Building(b)],
        });
        nearest(engaged.collect())
    }

    /// Whether the enemy regiment `e` a regiment is pursuing is no longer
    /// one to go after.
    fn lost(&self, r: usize, e: usize) -> bool {
        self.destroyed(e) || !self.hostile(r, Enemy::Regiment(e))
    }

    /// Calls off a regiment's pursuit, leaving it to stand where it is
    /// once done with any engagement.
    fn give_up(&mut self, r: usize) {
        let here = self.centre(r);
        let regiment = &mut self.regiments[r];
        regiment.order = Order::Move;
        regiment.resume = None;
        if regiment.engaged.is_empty() {
            let facing = regiment.facing;
            self.send(r, here, facing);
        }
    }

    /// Moves a regiment that isn't engaged on after the enemy regiment `e`
    /// it's pursuing: engaging it once it's in [`PURSUIT_REACH`], else
    /// turning after it as it gets away, and giving up where it stops
    /// short of it.
    fn pursue(&mut self, r: usize, e: usize) {
        if self.destroyed(r) {
            return;
        }
        let reach = PURSUIT_REACH * self.regiments[r].control_range;
        if within(self.centre(r), self.centre(e), reach) {
            let enemy = Enemy::Regiment(e);
            self.regiments[r].engaged.push(enemy);
            self.hold(r, enemy);
            return;
        }
        if !self.chase(r, e, false) && !self.underway(r) {
            self.give_up(r);
        }
    }

    /// Sends a regiment toward where the enemy regiment `e` stands on
    /// average, `afresh` or once it's drifted [`PURSUIT_DRIFT`] off where
    /// the regiment's headed. Returns whether it was sent.
    fn chase(&mut self, r: usize, e: usize, afresh: bool) -> bool {
        let (here, there) = (self.centre(r), self.centre(e));
        let off = squared_distance(self.regiments[r].position, there);
        if !afresh && off <= PURSUIT_DRIFT * squared_distance(here, there) {
            return false;
        }
        let facing = normalised([there[0] - here[0], there[1] - here[1]]);
        let facing = facing.unwrap_or(self.regiments[r].facing);
        self.send(r, there, facing);
        true
    }

    /// The enemies with something within a regiment's control range that
    /// its members can fire on.
    fn in_control(&self, r: usize) -> Vec<Enemy> {
        let here = self.centre(r);
        let reach = self.regiments[r].control_range;
        let fires_on = |t: Target| {
            self.survivors(r).any(|u| {
                let f = self.units[u].fighter.iter().flat_map(|f| &f.weapons);
                f.into_iter()
                    .any(|w| self.can_target(Target::Unit(u), w, t))
            })
        };
        let mut found = Vec::new();
        for t in self.targets() {
            let enemy = match t {
                Target::Unit(u) => Enemy::Regiment(self.units[u].regiment),
                Target::Building(b) => Enemy::Building(b),
            };
            if !found.contains(&enemy)
                && self.reach(here, t) <= reach
                && self.hostile(r, enemy)
                && fires_on(t)
            {
                found.push(enemy);
            }
        }
        found
    }

    fn hostile(&self, r: usize, enemy: Enemy) -> bool {
        let owner = match enemy {
            Enemy::Regiment(e) => self.regiments[e].owner,
            Enemy::Building(b) => self.buildings[b].owner,
        };
        self.sides.relation(self.regiments[r].owner, owner) == Relation::Enemy
    }

    /// Whether `enemy` is still one to `r` and in its operational range, or
    /// close enough to be run into where it has none.
    fn within_range(&self, r: usize, enemy: Enemy) -> bool {
        let reach = self.regiments[r].operational_range.max(CONTACT);
        let here = self.centre(r);
        let near = match enemy {
            Enemy::Regiment(e) => self
                .survivors(e)
                .any(|u| within(here, self.units[u].position, reach)),
            Enemy::Building(b) => {
                self.buildings[b].standing() && self.buildings[b].distance(here) <= reach
            }
        };
        near && self.hostile(r, enemy)
    }

    /// Stops a regiment that's just engaged where it stands, in its combat
    /// formation facing `enemy`, to carry on where it was going afterwards.
    fn hold(&mut self, r: usize, enemy: Enemy) {
        let here = self.centre(r);
        let there = match enemy {
            Enemy::Regiment(e) => self.centre(e),
            Enemy::Building(b) => self.buildings[b].position,
        };
        let regiment = &mut self.regiments[r];
        // A pursuit takes up its enemy afresh instead.
        if !matches!(regiment.order, Order::Pursue(_)) {
            regiment.resume = Some((regiment.position, regiment.facing));
        }
        let facing = normalised([there[0] - here[0], there[1] - here[1]]);
        let facing = facing.unwrap_or(regiment.facing);
        self.send(r, here, facing);
    }

    /// Sends a regiment that's no longer engaged on its way again, in the
    /// formation it was put in, or after the enemy it's pursuing.
    fn disengage(&mut self, r: usize) {
        if let Order::Pursue(e) = self.regiments[r].order {
            self.chase(r, e, true);
            return;
        }
        match self.regiments[r].resume.take() {
            Some((target, facing)) => self.send(r, target, facing),
            None => self.take_places(r),
        }
    }

    fn travel(&mut self) {
        for unit in &mut self.units {
            unit.previous = unit.position;
            unit.drive(self.regiments[unit.regiment].speed);
            if let Some(t) = &mut unit.trailer {
                t.previous = t.position;
                t.follow(unit.position, unit.facing);
            }
        }
    }

    /// Whatever can be fired on: alive units and standing buildings.
    fn targets(&self) -> impl Iterator<Item = Target> + '_ {
        let units = (0..self.units.len()).map(Target::Unit);
        let buildings = (0..self.buildings.len()).map(Target::Building);
        units
            .chain(buildings)
            .filter(|&t| self.fighter(t).is_some())
    }

    /// What a target can take and deal, if it's still there to fire on.
    fn fighter(&self, t: Target) -> Option<&Fighter> {
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

    fn owner(&self, t: Target) -> u32 {
        match t {
            Target::Unit(u) => self.regiments[self.units[u].regiment].owner,
            Target::Building(b) => self.buildings[b].owner,
        }
    }

    fn position(&self, t: Target) -> [f32; 2] {
        match t {
            Target::Unit(u) => self.units[u].position,
            Target::Building(b) => self.buildings[b].position,
        }
    }

    /// How far a point is from a target: to a building's edge, a unit's
    /// middle.
    fn reach(&self, from: [f32; 2], t: Target) -> f32 {
        match t {
            Target::Unit(u) => distance(from, self.units[u].position),
            Target::Building(b) => self.buildings[b].distance(from),
        }
    }

    /// Fires every weapon that's ready at what it can hit, then takes the
    /// dead out of the fight, so that all hit each other at once. Only
    /// turrets fire on the move: anything else turns its user to aim.
    /// Returns, for each unit hit, its regiment and what fired.
    fn fight(&mut self) -> Vec<(usize, Target)> {
        let mut hits: Vec<Hit> = Vec::new();
        let shooters: Vec<Target> = self.targets().collect();
        for s in shooters {
            let fighter = self.fighter(s).expect("shooters can fight");
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
            for (w, weapon) in fighter.weapons.iter().enumerate() {
                reloads[w] = (reloads[w] - TICK).max(0.0);
                if reloads[w] > 0.0 || moving && !weapon.turret {
                    continue;
                }
                let Some(t) = self.aim(s, weapon, target) else {
                    continue;
                };
                target = Some(t);
                shot = Some((self.ticks, w));
                reloads[w] = weapon.recharge;
                self.strike(weapon, self.position(t), t, (s, by), &mut hits);
            }
            match s {
                Target::Unit(u) => {
                    let u = &mut self.units[u];
                    (u.reloads, u.target, u.shot) = (reloads, target, shot);
                }
                Target::Building(b) => {
                    let b = &mut self.buildings[b];
                    (b.reloads, b.target, b.shot) = (reloads, target, shot);
                }
            }
        }
        let mut fired_on = Vec::new();
        for hit in hits {
            if let Target::Unit(u) = hit.target {
                fired_on.push((self.units[u].regiment, hit.from));
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

    /// Takes a hit's damage off what it lands on. A building whose health
    /// runs out is captured by the hit's owner if it can be, else destroyed
    /// if it can be, else disabled.
    fn land(&mut self, hit: Hit) {
        let ticks = self.ticks;
        match hit.target {
            Target::Unit(u) => {
                let unit = &mut self.units[u];
                if !unit.alive() {
                    return;
                }
                unit.health -= hit.amount;
                if unit.health <= 0.0 {
                    unit.die(ticks);
                }
            }
            Target::Building(b) => {
                let building = &mut self.buildings[b];
                // Nothing a side fires hurts its own, such as what it took
                // over earlier in the tick.
                if !building.standing() || building.owner == hit.by {
                    return;
                }
                building.health -= hit.amount;
                if building.health > 0.0 {
                    return;
                }
                building.health = 0.0;
                building.target = None;
                // Cities change hands by who holds them instead.
                if building.fighter.capturable && building.zone.is_none() {
                    building.health = CAPTURE_HEALTH * building.fighter.health;
                    self.change_hands(b, hit.by);
                } else if building.fighter.destructible {
                    building.state = State::Destroyed(ticks);
                    if let Some(o) = building.obstacle.take() {
                        self.pathing.unblock(&o);
                    }
                } else {
                    building.state = State::Disabled;
                }
            }
        }
    }

    /// Gives a building, and the rest of its city if it's one's centre, to
    /// `owner`.
    fn change_hands(&mut self, b: usize, owner: u32) {
        let members = self.buildings[b].members.clone();
        for i in std::iter::once(b).chain(members) {
            let building = &mut self.buildings[i];
            building.owner = owner;
            building.captured = Some(self.ticks);
            building.target = None;
            building.rally = None;
        }
    }

    /// What `s` fires `weapon` at: the best it can, armed enemies first,
    /// then what can be captured or destroyed, then the rest, and the
    /// nearest of them; or the one it fired on last as long as it's as good.
    /// A unit whose regiment is pursuing an enemy fires on that enemy
    /// first whenever it can.
    fn aim(&self, s: Target, weapon: &Weapon, last: Option<Target>) -> Option<Target> {
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

    fn can_hit(&self, s: Target, weapon: &Weapon, t: Target) -> bool {
        let d = self.reach(self.position(s), t);
        d <= weapon.range && d >= weapon.min_range && self.can_target(s, weapon, t)
    }

    /// Whether `weapon` fires on what `t` is, wherever it stands.
    fn can_target(&self, s: Target, weapon: &Weapon, t: Target) -> bool {
        let Some(fighter) = self.fighter(t).filter(|_| t != s) else {
            return false;
        };
        let side = match self.sides.relation(self.owner(s), self.owner(t)) {
            Relation::Enemy => "enemy",
            Relation::Ally | Relation::Itself => "friend",
            _ => "neutral",
        };
        let side = Id::from(side);
        weapon
            .targets
            .allow(fighter.flags.iter().chain(std::iter::once(&side)))
    }

    /// Records the damage a shot fired by `from` of `by`'s side, landing at
    /// `at` and aimed at `t`, does to what it hits.
    fn strike(
        &self,
        weapon: &Weapon,
        at: [f32; 2],
        t: Target,
        (from, by): (Target, u32),
        hits: &mut Vec<Hit>,
    ) {
        if weapon.area <= 0.0 {
            if let Some(f) = self.fighter(t) {
                hits.push(Hit {
                    target: t,
                    amount: damage(weapon, f, 1.0),
                    from,
                    by,
                });
            }
            return;
        }
        for target in self.targets() {
            let d = self.reach(at, target);
            if d > weapon.area {
                continue;
            }
            let f = self.fighter(target).expect("targets can fight");
            hits.push(Hit {
                target,
                amount: damage(weapon, f, falloff(weapon, d)),
                from,
                by,
            });
        }
    }

    /// Hands each city to whoever holds its zone, as long as the side
    /// holding it has none of its own there to keep it.
    fn occupy(&mut self) {
        for c in 0..self.buildings.len() {
            let destroyed = self.buildings[c].gone();
            let Some(zone) = self.buildings[c].zone.filter(|_| !destroyed) else {
                continue;
            };
            if let Some(winner) = self.holder(c, zone)
                && winner != self.buildings[c].owner
            {
                self.change_hands(c, winner);
            }
        }
    }

    /// Who should hold the city centred on `c`: its owner while any of its
    /// own are in the zone or no enemy of its is, else the side there with
    /// none of its enemies there and no ally there stronger or in danger.
    fn holder(&self, c: usize, zone: f32) -> Option<u32> {
        let centre = self.buildings[c].position;
        let owner = self.buildings[c].owner;
        // Armed occupants, each by its health's share.
        let occupants: Vec<(u32, f32)> = self
            .targets()
            .filter(|&t| t != Target::Building(c) && self.reach(centre, t) <= zone)
            .filter_map(|t| {
                let f = self.fighter(t).filter(|f| !f.weapons.is_empty())?;
                let health = match t {
                    Target::Unit(u) => self.units[u].health,
                    Target::Building(b) => self.buildings[b].health,
                };
                Some((self.owner(t), health / f.health))
            })
            .collect();
        let weight = |of: &dyn Fn(u32) -> bool| -> f32 {
            occupants
                .iter()
                .filter(|(o, _)| of(*o))
                .map(|(_, w)| w)
                .sum()
        };
        let enemy = |of: u32| move |o: u32| self.sides.relation(o, of) == Relation::Enemy;
        let own = weight(&|o| o == owner);
        if owner != 0 && (own > 0.0 || weight(&enemy(owner)) <= 0.0) {
            return Some(owner);
        }
        let players: Vec<u32> = self.sides.players.iter().map(|p| p.id).collect();
        let present = |p: u32| weight(&|o| o == p);
        let threat = |p: u32| weight(&|o| self.sides.relation(p, o) == Relation::Enemy);
        let winner = players.iter().copied().find(|&p| {
            threat(p) <= 0.0
                && present(p) > 0.0
                && !players.iter().any(|&q| {
                    q != p
                        && self.sides.relation(p, q) == Relation::Ally
                        && (present(p) < present(q) || threat(q) > 0.0)
                })
        });
        winner.or((owner != 0).then_some(owner))
    }
}

/// Damage a shot does to what it lands on, for the side that fired it.
struct Hit {
    target: Target,
    amount: f32,
    /// What fired it.
    from: Target,
    by: u32,
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
/// every hit does at least 1.
fn damage(weapon: &Weapon, target: &Fighter, scale: f32) -> f32 {
    weapon
        .damage
        .iter()
        .map(|d| {
            let resist = target.resists.get(&d.kind).copied().unwrap_or(0.0);
            ((d.amount * scale - target.defense.max(0.0)) * (1.0 - resist)).max(1.0)
        })
        .sum()
}

/// Where a deployment offset from a building standing at `position`
/// facing `facing` lies.
fn deploy_point(position: [f32; 2], facing: [f32; 2], offset: [f32; 2]) -> [f32; 2] {
    aaa_map::from_local(position, facing, offset)
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    libm::hypotf(a[0] - b[0], a[1] - b[1])
}

fn squared_distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
    dx * dx + dy * dy
}

/// Whether `a` and `b` are no further than `reach` apart, without the cost
/// of [`distance`], for checks made of many pairs every tick.
fn within(a: [f32; 2], b: [f32; 2], reach: f32) -> bool {
    squared_distance(a, b) <= reach * reach
}

/// An order to a regiment from a player that doesn't own it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotYours;

/// The players in a match and how they stand toward each other.
#[derive(Debug, Clone, Default)]
pub struct Sides {
    pub players: Vec<Player>,
    pub teams: Vec<Team>,
}

impl Sides {
    pub fn player(&self, id: u32) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }

    /// How the player `from` stands toward the player `to`, by their ids;
    /// neutral toward a player the match doesn't have.
    pub fn relation(&self, from: u32, to: u32) -> Relation {
        aaa_map::relation(&self.players, &self.teams, from, to)
    }

    /// The player a person plays by default: the first a person may.
    pub fn first_playable(&self) -> Option<&Player> {
        self.players.iter().find(|p| p.playable && !p.independent)
    }
}

fn normalised([x, y]: [f32; 2]) -> Option<[f32; 2]> {
    let length = libm::hypotf(x, y);
    (length > 1e-6).then(|| [x / length, y / length])
}

/// Turns `facing` toward `to`, both unit vectors, by up to `most` radians,
/// returning how far it turned.
fn steer(facing: &mut [f32; 2], to: [f32; 2], most: f32) -> f32 {
    let [fx, fy] = *facing;
    let angle = libm::atan2f(fx * to[1] - fy * to[0], fx * to[0] + fy * to[1]);
    if angle.abs() <= most {
        *facing = to;
        return angle.abs();
    }
    let (sin, cos) = libm::sincosf(most * angle.signum());
    *facing = [fx * cos - fy * sin, fx * sin + fy * cos];
    most
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use aaa_map::{Obstacle, Shape};
    use tgck::map::{Detail, Thing};

    fn member(id: u32, position: [f32; 2], speed: f32) -> aaa_map::Member {
        aaa_map::Member {
            thing: Thing {
                template: "soldier".into(),
                class: tgck::map::ACTOR,
                id,
                name: String::new(),
                position: [position[0], position[1], 0.0],
                facing: [0.0, 1.0, 0.0],
                flags: 0,
                scale: 1.0,
                variation: -1,
                seen_by: 0,
                detail: Detail::Raw(Vec::new()),
            },
            speed,
            shape: None,
            acceleration: 0.0,
            turn_rate: 0.0,
            trailer: None,
            fighter: None,
            health: 1.0,
        }
    }

    /// Two soldiers side by side facing +y, the slower one on the right,
    /// who can also line up one behind the other.
    fn world(pathing: Pathing) -> World {
        let formation = |id: &str, offsets| aaa_map::Formation {
            id: id.into(),
            offsets,
            aggressive: false,
        };
        let regiment = aaa_map::Regiment {
            id: 1,
            template: "squad".into(),
            owner: 7,
            facing: [0.0, 1.0],
            members: vec![member(2, [9.0, 10.0], 3.0), member(3, [11.0, 10.0], 2.0)],
            formations: vec![
                formation("line", vec![[-1.0, 0.0], [1.0, 0.0]]),
                formation("file", vec![[0.0, 1.0], [0.0, -1.0]]),
            ],
            formation: 0,
            combat: None,
            operational_range: None,
            control_range: None,
            primary: None,
            unpacks: None,
        };
        World::from_regiments(Sides::default(), pathing, &[regiment])
    }

    fn run(world: &mut World, seconds: f32) {
        for _ in 0..(seconds / TICK).round() as usize {
            world.tick();
        }
    }

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        distance(a, b) < 1e-3
    }

    #[test]
    fn regiments_move_at_their_slowest_members_speed() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        assert!(near(w.units[0].position, [9.0, 12.0]));
        assert!(near(w.units[1].position, [11.0, 12.0]));
        assert!(w.units.iter().all(Unit::moving));
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [9.0, 20.0]));
        assert!(!w.units.iter().any(Unit::moving));
    }

    /// A lone vehicle at 10,10 facing +y.
    fn vehicle(acceleration: f32, turn_rate: f32, trailer: Option<aaa_map::Trailer>) -> World {
        let mut m = member(2, [10.0, 10.0], 2.0);
        (m.acceleration, m.turn_rate, m.trailer) = (acceleration, turn_rate, trailer);
        let regiment = aaa_map::Regiment {
            id: 1,
            template: "tank".into(),
            owner: 7,
            facing: [0.0, 1.0],
            members: vec![m],
            formations: vec![aaa_map::Formation {
                id: "lone".into(),
                offsets: vec![[0.0, 0.0]],
                aggressive: false,
            }],
            formation: 0,
            combat: None,
            operational_range: None,
            control_range: None,
            primary: None,
            unpacks: None,
        };
        World::from_regiments(
            Sides::default(),
            Pathing::uniform(20, Domain::Land),
            &[regiment],
        )
    }

    #[test]
    fn units_speed_up_to_their_top_speed_and_stop_at_once() {
        let mut w = vehicle(2.0, 0.0, None);
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 30.0],
            },
        )
        .unwrap();
        // 0.2 a tick until it's at 2 a second after a second.
        run(&mut w, 1.0);
        assert!((w.units[0].speed - 2.0).abs() < 1e-4);
        assert!(
            near(w.units[0].position, [10.0, 11.1]),
            "{:?}",
            w.units[0].position
        );
        run(&mut w, 15.0);
        assert!(near(w.units[0].position, [10.0, 30.0]));
        assert_eq!(w.units[0].speed, 0.0);
    }

    #[test]
    fn units_that_turn_over_time_drive_round_corners() {
        let mut w = vehicle(0.0, 1.0, None);
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [20.0, 10.0],
            },
        )
        .unwrap();
        w.tick();
        // A tenth of a radian toward +x, having gone 0.2 that way.
        let u = &w.units[0];
        let (sin, cos) = libm::sincosf(0.1);
        assert!(near(u.facing, [sin, cos]), "{:?}", u.facing);
        assert!(near(u.position, [10.0 + 0.2 * sin, 10.0 + 0.2 * cos]));
        run(&mut w, 2.0);
        assert!(w.units[0].position[1] > 10.5, "it swings wide");
        run(&mut w, 20.0);
        let u = &w.units[0];
        assert!(!u.moving());
        assert!((u.position[0] - 20.0).abs() < 0.5, "{:?}", u.position);
    }

    #[test]
    fn trailers_follow_their_hitch_round_corners() {
        let trailer = aaa_map::Trailer {
            template: "gun".into(),
            hitch: [0.0, -2.0],
            coupling: [0.0, 1.0],
            placed: None,
        };
        let mut w = vehicle(0.0, 0.0, Some(trailer));
        let t = w.units[0].trailer.as_ref().unwrap();
        assert!(near(t.position, [10.0, 7.0]), "{:?}", t.position);
        assert!(near(t.facing, [0.0, 1.0]));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [20.0, 10.0],
            },
        )
        .unwrap();
        w.tick();
        // The truck turns to +x at once and goes 0.2: the hitch swings out
        // behind it to 8.2,10 and the trailer swings round after it.
        let t = w.units[0].trailer.as_ref().unwrap();
        let hitch = [8.2, 10.0];
        let coupled = aaa_map::from_local(t.position, t.facing, [0.0, 1.0]);
        assert!(near(coupled, hitch), "{coupled:?}");
        assert!(t.facing[0] < 0.0 && t.facing[1] > 0.0, "{:?}", t.facing);
        run(&mut w, 10.0);
        let t = w.units[0].trailer.as_ref().unwrap();
        assert!(near(t.facing, [1.0, 0.0]), "{:?}", t.facing);
        assert!(near(t.position, [17.0, 10.0]), "{:?}", t.position);
    }

    #[test]
    fn formations_turn_to_face_the_way_they_go() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        // Facing +x now, so the left-hand man ends up on the +y side.
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [30.0, 10.0],
            },
        )
        .unwrap();
        run(&mut w, 20.0);
        assert!(
            near(w.units[0].position, [30.0, 11.0]),
            "{:?}",
            w.units[0].position
        );
        assert!(near(w.units[1].position, [30.0, 9.0]));
        assert!(near(w.units[0].facing, [1.0, 0.0]));
    }

    #[test]
    fn members_go_back_to_their_places_after_being_kept_from_them() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        // The right-hand man's place at 11,20 is blocked.
        pathing.set_blocked(5, 10);
        let mut w = world(pathing);
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(!near(w.units[1].position, [11.0, 20.0]));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 30.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [9.0, 30.0]));
        assert!(near(w.units[1].position, [11.0, 30.0]));
    }

    #[test]
    fn changing_formation_rearranges_them_where_they_stand() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.apply(
            7,
            Command::Formation {
                regiment: 0,
                formation: 1,
            },
        )
        .unwrap();
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [10.0, 11.0]));
        assert!(near(w.units[1].position, [10.0, 9.0]));
        assert!(near(w.units[1].facing, [0.0, 1.0]));
    }

    #[test]
    fn units_go_around_what_blocks_them() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        for x in 0..15 {
            pathing.set_blocked(x, 8);
        }
        let mut w = world(pathing.clone());
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 30.0],
            },
        )
        .unwrap();
        for _ in 0..400 {
            w.tick();
            for u in &w.units {
                let (x, y) = tile_at(u.position);
                assert!(pathing.passable(x, y, Domain::Land), "{:?}", u.position);
            }
        }
        assert!(near(w.units[0].position, [9.0, 30.0]));
    }

    #[test]
    fn units_pass_through_scenery_they_stand_in() {
        // A rock around the left-hand man, blocking every tile near him.
        let rock = Obstacle {
            position: [9.0, 10.0],
            facing: [0.0, 1.0],
            shape: Shape::Circle { radius: 3.0 },
        };
        let mut pathing = Pathing::uniform(20, Domain::Land);
        pathing.block(&rock);
        let mut w = world(pathing);
        w.scenery.push(rock);
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 30.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [9.0, 30.0]));
    }

    #[test]
    fn units_pass_through_what_their_shape_overlaps() {
        // A wall right across the map, 2 ahead of them: it doesn't block
        // their tiles, but the left-hand man is big enough to overlap it.
        let wall = Obstacle {
            position: [20.0, 12.5],
            facing: [0.0, 1.0],
            shape: Shape::Rect {
                width: 40.0,
                height: 1.0,
            },
        };
        let mut pathing = Pathing::uniform(20, Domain::Land);
        pathing.block(&wall);
        let mut w = world(pathing);
        w.scenery.push(wall);
        w.units[0].shape = Some(Shape::Circle { radius: 2.5 });
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 30.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [9.0, 30.0]));
        assert!(w.units[1].position[1] < 12.0);
    }

    #[test]
    fn regiments_only_take_orders_from_their_owner() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        assert_eq!(w.order_move(8, 0, [10.0, 20.0]), Err(NotYours));
        assert_eq!(w.set_formation(8, 0, 1), Err(NotYours));
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [9.0, 10.0]));
        assert_eq!(w.regiments[0].formation, 0);
    }

    #[test]
    fn regiments_added_later_take_orders_like_the_first() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        let second = aaa_map::Regiment {
            id: 4,
            template: "squad".into(),
            owner: 7,
            facing: [0.0, 1.0],
            members: vec![member(5, [20.0, 10.0], 2.0)],
            formations: vec![aaa_map::Formation {
                id: "line".into(),
                offsets: vec![[0.0, 0.0]],
                aggressive: false,
            }],
            formation: 0,
            combat: None,
            operational_range: None,
            control_range: None,
            primary: None,
            unpacks: None,
        };
        let r = w.add_regiment(&second);
        assert_eq!((r, w.units[2].regiment), (1, 1));
        w.apply(
            7,
            Command::Move {
                regiment: r,
                to: [20.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[2].position, [20.0, 20.0]));
        assert!(near(w.units[0].position, [9.0, 10.0]));
    }

    #[test]
    fn destroyed_regiments_take_no_more_orders() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.destroy(0);
        assert!(w.destroyed(0));
        assert!(!w.commands(7, 0));
    }

    fn rifle(range: f32, amount: f32) -> Weapon {
        Weapon {
            range,
            min_range: 0.0,
            recharge: 2.0,
            area: 0.0,
            full_within: 1.0,
            edge_damage: 1.0,
            targets: aaa_map::Targets {
                required: vec![Id::from("enemy")],
                excluded: vec![Id::from("tank")],
            },
            damage: vec![aaa_map::Damage {
                kind: Id::from("gunfire"),
                amount,
            }],
            animation: None,
            turret: false,
            sounds: Vec::new(),
        }
    }

    fn soldier(weapons: Vec<Weapon>) -> Fighter {
        Fighter {
            health: 100.0,
            defense: 0.0,
            capturable: false,
            destructible: true,
            resists: HashMap::new(),
            flags: vec![Id::from("infantry")],
            weapons,
        }
    }

    /// A regiment for each of `owners`, of one fighter each, the first at
    /// 10,10 and each next 10 further along y.
    fn battle(owners: &[u32], fighters: Vec<Fighter>, sides: Sides) -> World {
        let regiments = owners
            .iter()
            .zip(fighters)
            .enumerate()
            .map(|(i, (&owner, fighter))| {
                let mut m = member(i as u32 * 2 + 2, [10.0, 10.0 + 10.0 * i as f32], 2.0);
                m.fighter = Some(fighter);
                aaa_map::Regiment {
                    id: i as u32 * 2 + 1,
                    template: "squad".into(),
                    owner,
                    facing: [0.0, 1.0],
                    members: vec![m],
                    formations: vec![aaa_map::Formation {
                        id: "line".into(),
                        offsets: vec![[0.0, 0.0]],
                        aggressive: false,
                    }],
                    formation: 0,
                    combat: None,
                    operational_range: Some(34.0),
                    control_range: Some(18.0),
                    primary: None,
                    unpacks: None,
                }
            })
            .collect::<Vec<_>>();
        World::from_regiments(sides, Pathing::uniform(40, Domain::Land), &regiments)
    }

    /// Players 1 and 2 at war, and 3 allied to 1.
    fn at_war() -> Sides {
        Sides {
            players: vec![
                player(1, None, &[0, 3, 1], &[]),
                player(2, None, &[3, 0, 3], &[]),
                player(3, None, &[1, 3, 0], &[]),
            ],
            teams: Vec::new(),
        }
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

    /// Player 1's strong rifleman at 10,10 and player 2's weak one at
    /// 10,30, out of each other's range; the first can also stand in a
    /// wider line, which it fights in.
    fn skirmish() -> World {
        let mut w = battle(
            &[1, 2],
            vec![
                soldier(vec![rifle(12.0, 50.0)]),
                soldier(vec![rifle(12.0, 10.0)]),
            ],
            at_war(),
        );
        w.units[1].position = [10.0, 30.0];
        let r = &mut w.regiments[0];
        r.formations.push(aaa_map::Formation {
            id: "wide".into(),
            offsets: vec![[0.0, 0.0]],
            aggressive: false,
        });
        r.combat = Some(1);
        w
    }

    #[test]
    fn moving_regiments_stop_to_fight_what_fires_on_them_then_carry_on() {
        let mut w = skirmish();
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        // In the enemy's range after 4s, at 18.
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert_eq!(w.regiments[0].current_formation(), 1);
        assert!(w.units[0].health < 100.0);
        let stopped = w.units[0].position;
        assert!(stopped[1] < 20.0, "{stopped:?}");
        // Orders to move only take effect once it's done.
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [30.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        assert!(near(w.units[0].position, stopped));
        assert!(near(w.units[0].facing, [0.0, 1.0]));
        run(&mut w, 2.0);
        assert!(!w.units[1].alive());
        assert!(w.regiments[0].engaged.is_empty());
        assert_eq!(w.regiments[0].current_formation(), 0);
        run(&mut w, 15.0);
        assert!(near(w.units[0].position, [30.0, 20.0]));
    }

    #[test]
    fn aggressive_regiments_engage_enemies_they_run_into() {
        let mut w = skirmish();
        w.regiments[0].formations[0].aggressive = true;
        // Too short a control range to see it coming.
        w.regiments[0].control_range = 1.0;
        // Unarmed, so it doesn't fire on them first.
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.units[0].fighter = Some(soldier(vec![rifle(2.0, 50.0)]));
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 9.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(!w.units[0].moving());
        assert!(distance(w.units[0].position, w.units[1].position) <= CONTACT);
        run(&mut w, 15.0);
        assert!(!w.units[1].alive());
        assert!(near(w.units[0].position, [10.0, 50.0]));
    }

    #[test]
    fn moving_regiments_engage_buildings_that_fire_on_them() {
        let mut pillbox = building(20, 2, [21.0, 20.0]);
        pillbox.fighter.destructible = true;
        pillbox.fighter.weapons = vec![rifle(12.0, 10.0)];
        let mut w = siege(&[pillbox]);
        w.units[0].fighter = Some(soldier(vec![rifle(12.0, 50.0)]));
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        // In its range from 15.2, after 2.6s.
        run(&mut w, 3.0);
        assert_eq!(w.regiments[0].engaged, [Enemy::Building(0)]);
        assert!(!w.units[0].moving());
        run(&mut w, 2.0);
        assert!(matches!(w.buildings[0].state, State::Destroyed(_)));
        assert!(w.regiments[0].engaged.is_empty());
        run(&mut w, 20.0);
        assert!(near(w.units[0].position, [10.0, 50.0]));
    }

    #[test]
    fn attacking_regiments_engage_what_comes_within_control_range_then_carry_on() {
        let mut w = skirmish();
        // Unarmed, so it never fires on them to draw them in.
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.apply(
            1,
            Command::Attack {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        assert_eq!(w.regiments[0].current_formation(), 1);
        // Within its control range of 18 from 12, after 1s; its rifle
        // reaches from 18.
        run(&mut w, 1.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(w.regiments[0].position[1] < 14.0);
        assert!(w.underway(0));
        run(&mut w, 6.0);
        assert!(!w.units[1].alive());
        run(&mut w, 25.0);
        assert!(near(w.regiments[0].position, [10.0, 50.0]));
        assert!(!w.underway(0));
        assert_eq!(w.regiments[0].order, Order::Move);
        assert_eq!(w.regiments[0].current_formation(), 0);
    }

    #[test]
    fn passive_regiments_only_answer_fire() {
        let mut w = skirmish();
        w.units[1].position = [10.0, 26.0];
        // The enemy's within its control range but out of its own reach.
        w.units[1].fighter = Some(soldier(vec![rifle(12.0, 10.0)]));
        w.units[0].fighter = Some(soldier(vec![rifle(12.0, 50.0)]));
        run(&mut w, 3.0);
        assert!(w.regiments[0].engaged.is_empty());
        w.regiments[0].formations[0].aggressive = true;
        w.tick();
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
    }

    #[test]
    fn attacking_regiments_leave_friends_alone() {
        let mut w = skirmish();
        w.regiments[1].owner = 1;
        w.apply(
            1,
            Command::Attack {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 6.0);
        assert!(w.regiments[0].engaged.is_empty());
        assert!(w.units[1].alive());
    }

    #[test]
    fn pursuing_regiments_engage_their_enemy_once_within_control_range() {
        let mut w = skirmish();
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.apply(
            1,
            Command::Pursue {
                regiment: 0,
                enemy: 1,
            },
        )
        .unwrap();
        assert_eq!(w.regiments[0].order, Order::Pursue(1));
        assert_eq!(w.regiments[0].current_formation(), 1);
        // Away to 38 by 4s; within its control range of 18 after 5s.
        w.apply(
            2,
            Command::Move {
                regiment: 1,
                to: [10.0, 38.0],
            },
        )
        .unwrap();
        run(&mut w, 4.5);
        assert!(w.regiments[0].engaged.is_empty());
        assert!(w.units[0].moving());
        run(&mut w, 1.0);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(w.regiments[0].position[1] < 22.0);
        run(&mut w, 6.0);
        assert!(!w.units[1].alive());
        assert_eq!(w.regiments[0].order, Order::Move);
        // It forms up again where it engaged, and goes no further.
        let stopped = w.regiments[0].position;
        run(&mut w, 5.0);
        assert!(near(w.regiments[0].position, stopped));
        assert!(!w.underway(0));
    }

    #[test]
    fn pursuing_regiments_engage_what_they_cant_fire_on_once_near_enough() {
        let mut w = skirmish();
        // Nothing of the pursuer's fires on it, so it never draws it in.
        w.units[1].fighter = Some(Fighter {
            flags: vec![Id::from("tank")],
            ..soldier(Vec::new())
        });
        w.apply(
            1,
            Command::Pursue {
                regiment: 0,
                enemy: 1,
            },
        )
        .unwrap();
        // Within 0.6 of its control range of 18, from 19.2, after 4.6s.
        run(&mut w, 4.0);
        assert!(w.regiments[0].engaged.is_empty());
        run(&mut w, 1.0);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!((18.5..20.5).contains(&w.units[0].position[1]));
    }

    #[test]
    fn engaged_members_close_in_on_what_they_cant_reach() {
        let mut w = skirmish();
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.units[0].fighter = Some(soldier(vec![rifle(4.0, 50.0)]));
        w.apply(
            1,
            Command::Pursue {
                regiment: 0,
                enemy: 1,
            },
        )
        .unwrap();
        // Engaged from 19.2, after 4.6s, then closing in alone to 26.
        run(&mut w, 5.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(w.units[0].moving());
        assert!(w.regiments[0].position[1] < 21.0);
        run(&mut w, 4.0);
        assert!(distance(w.units[0].position, w.units[1].position) <= 4.0);
        assert!(w.units[1].health < 100.0);
        assert!(w.regiments[0].position[1] < 21.0);
    }

    #[test]
    fn pursuing_regiments_fight_what_fires_on_them_then_carry_on() {
        let mut w = battle(
            &[1, 2, 2],
            vec![
                soldier(vec![rifle(12.0, 50.0)]),
                soldier(vec![rifle(12.0, 10.0)]),
                soldier(Vec::new()),
            ],
            at_war(),
        );
        w.units[1].position = [20.0, 10.0];
        w.units[2].position = [10.0, 38.0];
        w.apply(
            1,
            Command::Pursue {
                regiment: 0,
                enemy: 2,
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(!w.units[0].moving());
        run(&mut w, 3.0);
        assert!(!w.units[1].alive());
        assert!(w.regiments[0].engaged.is_empty());
        assert!(w.units[0].moving());
        assert_eq!(w.regiments[0].order, Order::Pursue(2));
        run(&mut w, 15.0);
        assert!(!w.units[2].alive());
    }

    #[test]
    fn pursuing_regiments_fire_on_their_enemy_first() {
        let mut w = battle(
            &[1, 2, 2],
            vec![
                soldier(vec![rifle(12.0, 50.0)]),
                soldier(vec![rifle(12.0, 10.0)]),
                soldier(Vec::new()),
            ],
            at_war(),
        );
        // Armed and nearer, but not the one pursued.
        w.units[1].position = [16.0, 10.0];
        w.units[2].position = [10.0, 20.0];
        w.apply(
            1,
            Command::Pursue {
                regiment: 0,
                enemy: 2,
            },
        )
        .unwrap();
        run(&mut w, 0.5);
        assert_eq!(w.units[0].target, Some(Target::Unit(2)));
        run(&mut w, 2.0);
        assert!(!w.units[2].alive());
        assert_eq!(w.units[1].health, 100.0);
        run(&mut w, 4.0);
        assert!(!w.units[1].alive());
    }

    #[test]
    fn pursuing_regiments_give_up_on_what_they_cant_reach() {
        let mut w = skirmish();
        w.units[1].fighter = Some(soldier(Vec::new()));
        for x in 0..40 {
            w.pathing.set_blocked(x, 10);
        }
        w.apply(
            1,
            Command::Pursue {
                regiment: 0,
                enemy: 1,
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(w.regiments[0].engaged.is_empty());
        assert_eq!(w.regiments[0].order, Order::Move);
        assert!(!w.underway(0));
        assert!(w.units[0].position[1] < 20.0);
    }

    #[test]
    fn regiments_only_pursue_enemies_and_only_when_armed() {
        let mut w = skirmish();
        w.regiments[1].owner = 1;
        assert_eq!(w.order_pursue(1, 0, 1), Err(Refused::Unable));
        let mut w = skirmish();
        w.units[0].fighter = Some(soldier(Vec::new()));
        assert_eq!(w.order_pursue(1, 0, 1), Err(Refused::Unable));
        assert_eq!(w.order_pursue(2, 0, 1), Err(Refused::NotYours));
    }

    #[test]
    fn running_regiments_break_off_and_ignore_enemies() {
        let mut w = skirmish();
        // Too slow to keep after it.
        w.regiments[1].speed = 0.5;
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        w.apply(
            1,
            Command::Run {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        assert_eq!(w.regiments[0].order, Order::Run);
        for _ in 0..200 {
            w.tick();
            assert!(w.regiments[0].engaged.is_empty());
        }
        assert!(w.units[0].alive() && w.units[1].alive());
        assert!(near(w.units[0].position, [10.0, 50.0]));
        assert_eq!(w.regiments[0].order, Order::Move);
    }

    #[test]
    fn stopped_regiments_form_up_where_they_are() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        w.apply(7, Command::Stop { regiment: 0 }).unwrap();
        run(&mut w, 5.0);
        assert!(near(w.regiments[0].position, [10.0, 12.0]));
        assert!(near(w.units[0].position, [9.0, 12.0]));
        assert!(near(w.units[1].position, [11.0, 12.0]));
        assert!(!w.units.iter().any(Unit::moving));
        assert!(w.order_stop(1, 0).is_err());
    }

    #[test]
    fn regiments_are_underway_until_they_arrive() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        assert!(!w.underway(0));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        assert!(w.underway(0));
        assert!(near(w.destination(0), [10.0, 20.0]));
        run(&mut w, 30.0);
        assert!(!w.underway(0));
        w.apply(
            7,
            Command::Move {
                regiment: 0,
                to: [10.0, 5.0],
            },
        )
        .unwrap();
        run(&mut w, 0.5);
        w.apply(7, Command::Stop { regiment: 0 }).unwrap();
        assert!(!near(w.destination(0), [10.0, 5.0]));
    }

    #[test]
    fn fighting_on_the_way_keeps_the_destination() {
        let mut w = skirmish();
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(w.underway(0));
        assert!(near(w.destination(0), [10.0, 50.0]));
    }

    #[test]
    fn stopped_regiments_stay_put_after_fighting() {
        let mut w = skirmish();
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 50.0],
            },
        )
        .unwrap();
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        let held = w.regiments[0].position;
        w.apply(1, Command::Stop { regiment: 0 }).unwrap();
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        w.destroy(1);
        run(&mut w, 10.0);
        assert!(w.regiments[0].engaged.is_empty());
        assert!(near(w.regiments[0].position, held));
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
        assert!((damage(&rifle(12.0, 12.0), &tank, 1.0) - 2.4).abs() < 1e-5);
        assert_eq!(damage(&rifle(12.0, 5.0), &tank, 1.0), 1.0);
        assert!((damage(&rifle(12.0, 50.0), &tank, 0.5) - 7.6).abs() < 1e-5);
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

    /// An unarmed building of `owner`'s with 100 health, 6 across and
    /// square to the axes, at `position`.
    fn building(id: u32, owner: u32, position: [f32; 2]) -> aaa_map::Building {
        aaa_map::Building {
            id,
            template: "house".into(),
            owner,
            position,
            facing: [0.0, -1.0],
            fighter: Fighter {
                flags: vec![Id::from("building")],
                destructible: false,
                ..soldier(Vec::new())
            },
            health: 1.0,
            zone: None,
            members: Vec::new(),
            obstacle: Some(Obstacle {
                position,
                facing: [0.0, -1.0],
                shape: Shape::Rect {
                    width: 6.0,
                    height: 6.0,
                },
            }),
            deploy: None,
        }
    }

    /// Player 1's rifleman at 10,10 and whatever buildings, on open ground.
    fn siege(buildings: &[aaa_map::Building]) -> World {
        let mut w = battle(&[1], vec![soldier(vec![rifle(12.0, 30.0)])], at_war());
        for b in buildings {
            if let Some(o) = &b.obstacle {
                w.pathing.block(o);
            }
        }
        w.add_buildings(buildings);
        w
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
    fn armed_enemies_come_before_buildings() {
        // The house is nearer, but the enemy soldier can fire back.
        let mut w = battle(
            &[1, 2],
            vec![
                soldier(vec![rifle(12.0, 30.0)]),
                soldier(vec![rifle(12.0, 1.0)]),
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

    #[test]
    fn buildings_out_of_health_are_captured_destroyed_or_disabled() {
        let mut bunker = building(20, 2, [10.0, 20.0]);
        bunker.fighter.capturable = true;
        let mut pillbox = building(21, 2, [20.0, 10.0]);
        pillbox.fighter.destructible = true;
        let house = building(22, 2, [0.0, 10.0]);
        let mut w = siege(&[bunker, pillbox, house]);
        assert!(!w.pathing.passable(10, 5, Domain::Land));
        run(&mut w, 30.0);
        // Its new owner's rifle leaves it be at the share it's left with.
        let b = &w.buildings[0];
        assert_eq!((b.owner, b.state), (1, State::Standing));
        assert!((b.health - 30.0).abs() < 1e-3);
        assert!(b.captured.is_some());
        assert!(matches!(w.buildings[1].state, State::Destroyed(_)));
        assert!(w.pathing.passable(10, 5, Domain::Land));
        assert_eq!(w.buildings[2].state, State::Disabled);
        assert_eq!(w.buildings[2].owner, 2);
        assert_eq!(w.units[0].target, None);
    }

    /// Player 2's city centred at 40,10 with a zone of 15 and a house of
    /// its own, out of the rifleman's reach.
    fn city() -> [aaa_map::Building; 2] {
        let mut centre = building(30, 2, [40.0, 10.0]);
        centre.fighter.capturable = true;
        centre.zone = Some(15.0);
        centre.members = vec![31];
        [centre, building(31, 2, [40.0, 20.0])]
    }

    #[test]
    fn cities_go_to_whoever_holds_their_zone() {
        let mut w = siege(&city());
        run(&mut w, 1.0);
        assert_eq!(w.buildings[0].owner, 2);
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [30.0, 10.0],
            },
        )
        .unwrap();
        run(&mut w, 15.0);
        assert!(w.buildings.iter().all(|b| b.owner == 1));
        assert!(w.buildings[0].captured.is_some());
        // Leaving doesn't give it back.
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 10.0],
            },
        )
        .unwrap();
        run(&mut w, 15.0);
        assert!(w.buildings.iter().all(|b| b.owner == 1));
    }

    #[test]
    fn cities_hold_out_while_their_own_are_there() {
        let mut w = battle(
            &[1, 2],
            vec![
                soldier(vec![rifle(1.0, 30.0)]),
                soldier(vec![rifle(1.0, 30.0)]),
            ],
            at_war(),
        );
        w.add_buildings(&city());
        // Player 2's soldier inside the zone, out of the other's reach.
        w.units[1].position = [40.0, 15.0];
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [30.0, 10.0],
            },
        )
        .unwrap();
        run(&mut w, 15.0);
        assert_eq!(w.buildings[0].owner, 2);
        w.units[1].died = Some(w.ticks);
        w.tick();
        assert_eq!(w.buildings[0].owner, 1);
    }

    fn player(id: u32, team: Option<usize>, relations: &[u32], team_relations: &[u32]) -> Player {
        let stored = |r: &[u32]| r.iter().map(|&r| Relation::from_u32(r).unwrap()).collect();
        Player {
            id,
            name: format!("kingdom_{id}"),
            color: String::new(),
            rgb: [1.0; 3],
            team,
            independent: team.is_none(),
            playable: team.is_some(),
            nation: None,
            relations: stored(relations),
            team_relations: stored(team_relations),
        }
    }

    #[test]
    fn teams_decide_relations_for_their_players() {
        let team = |id, relations: [Relation; 4], team_relations| Team {
            id,
            name: String::new(),
            relations: relations.into(),
            team_relations,
        };
        use Relation::{Ally, Enemy, Neutral};
        // Two players a team, at odds with each other as they're stored,
        // and civilians neutral to everyone.
        let sides = Sides {
            players: vec![
                player(1, Some(0), &[0, 3, 3, 3], &[3, 3]),
                player(2, Some(0), &[3, 0, 3, 3], &[3, 3]),
                player(3, Some(1), &[3, 3, 0, 3], &[3, 3]),
                player(4, None, &[2, 2, 2, 0], &[2, 2]),
            ],
            teams: vec![
                team(10, [Enemy, Enemy, Enemy, Neutral], vec![Ally, Enemy]),
                team(11, [Enemy, Enemy, Enemy, Neutral], vec![Enemy, Ally]),
            ],
        };
        assert_eq!(sides.relation(1, 1), Relation::Itself);
        assert_eq!(sides.relation(1, 2), Ally);
        assert_eq!(sides.relation(2, 3), Enemy);
        assert_eq!(sides.relation(3, 4), Neutral);
        assert_eq!(sides.relation(4, 1), Neutral);
        assert_eq!(sides.relation(1, 99), Neutral);
        assert_eq!(sides.first_playable().map(|p| p.id), Some(1));
    }

    #[test]
    fn players_without_teams_keep_their_own_relations() {
        let sides = Sides {
            players: vec![
                player(1, None, &[0, 1, 3], &[]),
                player(2, None, &[1, 0, 3], &[]),
                player(3, None, &[2, 3, 0], &[]),
            ],
            teams: Vec::new(),
        };
        assert_eq!(sides.relation(1, 2), Relation::Ally);
        assert_eq!(sides.relation(1, 3), Relation::Enemy);
        assert_eq!(sides.relation(3, 1), Relation::Neutral);
    }

    /// Everything about a match that orders and ticks can change.
    fn snapshot(w: &World) -> String {
        let units: Vec<_> = w
            .units
            .iter()
            .map(|u| (u.position, u.facing, u.health, u.target, u.shot, u.died))
            .collect();
        let regiments: Vec<_> = w
            .regiments
            .iter()
            .map(|r| (r.position, r.facing, r.formation, r.order, &r.engaged))
            .collect();
        let buildings: Vec<_> = w
            .buildings
            .iter()
            .map(|b| (b.owner, b.health, b.target, b.state, b.captured))
            .collect();
        format!("{:?}", (w.ticks, units, regiments, buildings))
    }

    /// Four regiments of riflemen moving, running and fighting around a
    /// house, played out to the end.
    fn played_battle() -> World {
        let rifles = |n| (0..n).map(|_| soldier(vec![rifle(12.0, 25.0)])).collect();
        let mut w = battle(&[1, 2, 3, 2], rifles(4), at_war());
        let house = building(20, 2, [30.0, 10.0]);
        w.pathing.block(house.obstacle.as_ref().unwrap());
        w.add_buildings(&[house]);
        let to = [30.0, 30.0];
        w.apply(1, Command::Move { regiment: 0, to }).unwrap();
        let to = [10.0, 50.0];
        w.apply(3, Command::Run { regiment: 2, to }).unwrap();
        run(&mut w, 15.0);
        let to = [20.0, 20.0];
        w.apply(2, Command::Move { regiment: 3, to }).unwrap();
        run(&mut w, 30.0);
        w
    }

    #[test]
    fn the_same_orders_play_out_the_same_way() {
        let (a, b) = (played_battle(), played_battle());
        assert!(a.units.iter().any(|u| u.died.is_some()));
        assert_eq!(snapshot(&a), snapshot(&b));
    }

    /// A match's numbers hashed with FNV-1a, which unlike std's hashers is
    /// fixed for good, and which unlike hashing its Debug output doesn't
    /// change when something's renamed.
    fn fingerprint(worlds: &[&World]) -> u64 {
        let f = |x: &f32| u64::from(x.to_bits());
        let mut numbers = Vec::new();
        for w in worlds {
            numbers.push(w.ticks);
            for u in &w.units {
                numbers.extend(u.position.iter().chain(&u.facing).map(f));
                numbers.extend([f(&u.health), u.died.unwrap_or(u64::MAX)]);
                if let Some(t) = &u.trailer {
                    numbers.extend(t.position.iter().chain(&t.facing).map(f));
                }
            }
            for r in &w.regiments {
                numbers.extend(r.position.iter().chain(&r.facing).map(f));
                numbers.push(r.formation as u64);
            }
            for b in &w.buildings {
                numbers.extend([u64::from(b.owner), f(&b.health)]);
                numbers.push(b.captured.unwrap_or(u64::MAX));
            }
        }
        numbers
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .fold(0xcbf2_9ce4_8422_2325, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
            })
    }

    /// CI runs this on each platform it tests, so a match that plays out
    /// differently on one, as the platforms' own float functions can make
    /// it, fails there. The fingerprint changes whenever the rules do, and
    /// is then updated to the new one.
    #[test]
    fn matches_play_out_the_same_on_every_platform() {
        let trailer = aaa_map::Trailer {
            template: "gun".into(),
            hitch: [0.0, -2.0],
            coupling: [0.0, 1.0],
            placed: None,
        };
        // Towing round corners and a rock, which steering and going round
        // obstacles do with trigonometry and distances to their shapes.
        let mut towing = vehicle(2.0, 1.0, Some(trailer));
        let rock = Obstacle {
            position: [13.0, 13.0],
            facing: [0.0, 1.0],
            shape: Shape::Circle { radius: 1.5 },
        };
        towing.pathing.block(&rock);
        towing.scenery.push(rock);
        for to in [[16.0, 16.0], [4.0, 14.0], [12.0, 4.0]] {
            towing.apply(7, Command::Move { regiment: 0, to }).unwrap();
            run(&mut towing, 8.0);
        }
        let battle = played_battle();
        let played = format!("{}\n{}", snapshot(&battle), snapshot(&towing));
        assert_eq!(
            fingerprint(&[&battle, &towing]),
            0xe9cb6e6bcdf9ca03,
            "{played}"
        );
    }

    #[test]
    fn the_sim_and_map_forbid_the_same_float_functions() {
        let map = include_str!("../../map/clippy.toml");
        assert_eq!(include_str!("../clippy.toml"), map);
    }

    #[test]
    fn commands_are_refused_as_checked_without_being_carried_out() {
        let mut w = headquarters();
        let truck = Commission::new(truck(), 1.0);
        let refused = [
            (
                2,
                Command::Rally {
                    building: 0,
                    at: None,
                },
                Refused::NotYours,
            ),
            (1, Command::Pack { building: 0 }, Refused::Unable),
            (
                1,
                Command::CancelCommission {
                    building: 0,
                    commission: 0,
                },
                Refused::Unable,
            ),
            // What isn't there is refused, not a panic.
            (1, Command::Stop { regiment: 9 }, Refused::NotYours),
            (1, Command::Pack { building: 9 }, Refused::NotYours),
        ];
        for (player, command, why) in refused {
            assert_eq!(w.check(player, &command), Err(why), "{command:?}");
            assert_eq!(w.apply(player, command.clone()), Err(why), "{command:?}");
        }
        let command = Command::Commission {
            building: 0,
            commission: Box::new(truck),
        };
        assert_eq!(w.check(1, &command), Ok(()));
        // Checking alone changes nothing.
        assert!(w.buildings[0].queue.is_empty());
        w.apply(1, command).unwrap();
        assert_eq!(w.buildings[0].queue.len(), 1);
    }

    /// Player 1's HQ at 20,20, putting out what it commissions 6 to its
    /// left, on open ground with nothing else.
    fn headquarters() -> World {
        let mut hq = building(5, 1, [20.0, 20.0]);
        hq.deploy = Some(DeployPoints {
            creation: [0.0, 0.0],
            deployment: [-6.0, 0.0],
        });
        let mut w = World::from_regiments(at_war(), Pathing::uniform(40, Domain::Land), &[]);
        w.pathing.block(hq.obstacle.as_ref().unwrap());
        w.add_buildings(&[hq]);
        w
    }

    /// A truck that unpacks into an 8-wide depot, standing at the origin.
    fn truck() -> aaa_map::Regiment {
        let mut depot = building(0, 0, [0.0, 0.0]);
        depot.template = "depot".into();
        depot.obstacle.as_mut().unwrap().shape = Shape::Rect {
            width: 8.0,
            height: 8.0,
        };
        let mut m = member(0, [0.0, 0.0], 3.0);
        m.thing.template = "truck".into();
        m.fighter = Some(soldier(Vec::new()));
        aaa_map::Regiment {
            id: 0,
            template: "truck".into(),
            owner: 0,
            facing: [0.0, 1.0],
            members: vec![m],
            formations: vec![aaa_map::Formation {
                id: String::new(),
                offsets: vec![[0.0, 0.0]],
                aggressive: false,
            }],
            formation: 0,
            combat: None,
            operational_range: None,
            control_range: None,
            primary: Some(0),
            unpacks: Some(Unpacks {
                building: depot,
                room: Some(Shape::Rect {
                    width: 12.0,
                    height: 12.0,
                }),
                offset: [0.0, 0.0],
                seconds: 7.0,
            }),
        }
    }

    #[test]
    fn commissions_come_out_of_their_building_in_turn() {
        let mut w = headquarters();
        assert_eq!(
            w.commission(2, 0, Commission::new(truck(), 10.0)),
            Err(Refused::NotYours)
        );
        w.apply(
            1,
            Command::Commission {
                building: 0,
                commission: Box::new(Commission::new(truck(), 10.0)),
            },
        )
        .unwrap();
        w.apply(
            1,
            Command::Commission {
                building: 0,
                commission: Box::new(Commission::new(truck(), 10.0)),
            },
        )
        .unwrap();
        run(&mut w, 9.0);
        assert!(w.regiments.is_empty());
        run(&mut w, 1.0);
        assert_eq!(w.regiments.len(), 1);
        assert_eq!(w.buildings[0].queue.len(), 1);
        let r = &w.regiments[0];
        assert_eq!(r.owner, 1);
        // New ids, the lone truck's its regiment's too.
        assert_eq!((r.id, w.units[0].id), (6, 6));
        // It drives out to the HQ's left, which facing -y is +x.
        assert!(near(w.units[0].position, [20.0, 20.0]));
        run(&mut w, 3.0);
        assert!(near(w.units[0].position, [26.0, 20.0]));
        // The first is out, so calling it off is refused; the second isn't.
        let cancel = |commission| Command::CancelCommission {
            building: 0,
            commission,
        };
        assert_eq!(w.apply(1, cancel(0)), Err(Refused::Unable));
        w.apply(1, cancel(1)).unwrap();
        run(&mut w, 10.0);
        assert_eq!(w.regiments.len(), 1);
    }

    #[test]
    fn commissions_need_their_owners_required_buildings() {
        let mut w = headquarters();
        let ids = |ids: &[&str]| ids.iter().map(|&id| Id::from(id)).collect();
        w.requires.insert(
            Id::from("TRUCK"),
            Requires {
                structures: vec![ids(&["pool", "old_pool"]), ids(&["depot"])],
                any: false,
            },
        );
        let commission = || Command::Commission {
            building: 0,
            commission: Box::new(Commission::new(truck(), 1.0)),
        };
        let lacks = |w: &World| -> Vec<String> {
            w.lacks(1, &truck())
                .iter()
                .map(|id| id.to_string())
                .collect()
        };
        assert_eq!(lacks(&w), ["pool", "depot"]);
        assert_eq!(w.check(1, &commission()), Err(Refused::Lacking));
        assert_eq!(w.apply(1, commission()), Err(Refused::Lacking));
        // An enemy's doesn't count; one under a name it once had does.
        let mut theirs = building(6, 2, [5.0, 5.0]);
        theirs.template = "pool".into();
        let mut ours = building(7, 1, [35.0, 5.0]);
        ours.template = "Old_Pool".into();
        w.add_buildings(&[theirs, ours]);
        assert_eq!(lacks(&w), ["depot"]);
        w.requires.get_mut(&Id::from("truck")).unwrap().any = true;
        assert!(lacks(&w).is_empty());
        w.apply(1, commission()).unwrap();
        // Losing it stops further commissions.
        w.buildings[2].state = State::Destroyed(0);
        assert_eq!(w.apply(1, commission()), Err(Refused::Lacking));
    }

    #[test]
    fn commissions_put_out_onto_a_neighbours_ground_can_leave_it() {
        // A depot beside the HQ covers where it puts out what it makes,
        // and a house stands in the way out.
        let mut w = headquarters();
        let depot = building(6, 1, [28.0, 20.0]);
        let house = building(7, 1, [26.0, 12.0]);
        for b in [&depot, &house] {
            w.pathing.block(b.obstacle.as_ref().unwrap());
        }
        w.add_buildings(&[depot, house]);
        w.apply(
            1,
            Command::Commission {
                building: 0,
                commission: Box::new(Commission::new(truck(), 1.0)),
            },
        )
        .unwrap();
        run(&mut w, 4.0);
        assert!(near(w.units[0].position, [26.0, 20.0]));
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [26.0, 4.0],
            },
        )
        .unwrap();
        for _ in 0..300 {
            w.tick();
            assert!(w.buildings[2].distance(w.units[0].position) > 0.0);
        }
        assert!(near(w.units[0].position, [26.0, 4.0]));
    }

    #[test]
    fn commissions_move_on_to_the_rally_point() {
        let mut w = headquarters();
        assert_eq!(
            w.set_rally(2, 0, Some([26.0, 30.0])),
            Err(Refused::NotYours)
        );
        w.apply(
            1,
            Command::Rally {
                building: 0,
                at: Some([26.0, 30.0]),
            },
        )
        .unwrap();
        w.apply(
            1,
            Command::Commission {
                building: 0,
                commission: Box::new(Commission::new(truck(), 1.0)),
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        // Out to its deployment point first, then on to the rally point.
        assert_eq!(w.destination(0), [26.0, 20.0]);
        run(&mut w, 3.0);
        assert_eq!(w.destination(0), [26.0, 30.0]);
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [26.0, 30.0]));
        assert!(w.regiments[0].rally.is_none());
        // An order on the way calls it off.
        w.apply(
            1,
            Command::Commission {
                building: 0,
                commission: Box::new(Commission::new(truck(), 1.0)),
            },
        )
        .unwrap();
        run(&mut w, 2.0);
        w.apply(1, Command::Stop { regiment: 1 }).unwrap();
        run(&mut w, 8.0);
        assert!(w.units[1].position[1] < 25.0);
        // Without one, they stay where they come out.
        w.apply(
            1,
            Command::Rally {
                building: 0,
                at: None,
            },
        )
        .unwrap();
        w.apply(
            1,
            Command::Commission {
                building: 0,
                commission: Box::new(Commission::new(truck(), 1.0)),
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[2].position, [26.0, 20.0]));
        let mut house = siege(&[building(9, 1, [10.0, 30.0])]);
        assert_eq!(
            house.set_rally(1, 0, Some([0.0, 0.0])),
            Err(Refused::Unable)
        );
    }

    #[test]
    fn trucks_unpack_where_theres_room_once_there() {
        let mut w = headquarters();
        let mut t = truck();
        t.owner = 1;
        t.renumber(9);
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        // Over the HQ, too close beside it, and over the map's edge.
        assert_eq!(w.order_unpack(1, r, [22.0, 22.0]), Err(Refused::NoRoom));
        assert_eq!(w.order_unpack(1, r, [28.0, 20.0]), Err(Refused::NoRoom));
        assert_eq!(w.order_unpack(1, r, [78.0, 30.0]), Err(Refused::NoRoom));
        assert_eq!(w.order_unpack(2, r, [30.0, 40.0]), Err(Refused::NotYours));
        assert_eq!(w.order_unpack(1, r, [30.2, 39.6]), Ok([30.0, 40.0]));
        run(&mut w, 3.0);
        assert!(w.units[0].alive());
        assert_eq!(w.buildings.len(), 1);
        run(&mut w, 1.0);
        assert!(!w.units[0].alive());
        assert!(w.units[0].died.is_none());
        assert!(w.destroyed(r));
        let depot = &w.buildings[1];
        assert_eq!((depot.template.as_str(), depot.owner), ("depot", 1));
        assert_eq!(depot.position, [30.0, 40.0]);
        let (from, to) = depot.unpacking.unwrap();
        assert_eq!(to - from, 70);
        assert!(!w.pathing.passable(15, 20, Domain::Land));
        run(&mut w, 7.0);
        assert!(w.buildings[1].unpacking.is_none());
    }

    #[test]
    fn other_orders_call_off_unpacking() {
        let mut w = headquarters();
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        w.apply(
            1,
            Command::Unpack {
                regiment: r,
                at: [30.0, 40.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        w.apply(1, Command::Stop { regiment: r }).unwrap();
        run(&mut w, 5.0);
        assert!(w.units[0].alive());
        assert_eq!(w.buildings.len(), 1);
    }

    #[test]
    fn buildings_pack_back_into_their_trucks() {
        let mut w = headquarters();
        w.packing.insert(
            "depot".into(),
            Packs {
                truck: truck(),
                offset: [0.0, 0.0],
                facing: [1.0, 0.0],
                seconds: 7.0,
                health_min: 0.5,
            },
        );
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        w.apply(
            1,
            Command::Unpack {
                regiment: r,
                at: [30.0, 40.0],
            },
        )
        .unwrap();
        run(&mut w, 4.0);
        let depot = w.regiments[r].unpacked_into.unwrap();
        // Not while it's unpacking, nor someone else's, nor what packs to
        // nothing.
        assert_eq!(w.order_pack(1, depot), Err(Refused::Unable));
        run(&mut w, 7.0);
        assert_eq!(w.order_pack(2, depot), Err(Refused::NotYours));
        assert_eq!(w.order_pack(1, 0), Err(Refused::Unable));
        let full = w.buildings[depot].health;
        w.buildings[depot].health = full * 0.5;
        assert_eq!(w.order_pack(1, depot), Err(Refused::Unable));
        w.buildings[depot].health = full * 0.8;
        w.apply(1, Command::Pack { building: depot }).unwrap();
        run(&mut w, 6.9);
        assert!(w.buildings[depot].standing());
        assert!(!w.pathing.passable(15, 20, Domain::Land));
        run(&mut w, 0.2);
        let State::Packed(_, packed) = w.buildings[depot].state else {
            panic!("not packed: {:?}", w.buildings[depot].state);
        };
        assert!(w.buildings[depot].gone());
        assert!(w.pathing.passable(15, 20, Domain::Land));
        let u = w.regiments[packed].units[0];
        assert_eq!(w.regiments[packed].owner, 1);
        assert!(near(w.units[u].position, [30.0, 40.0]));
        assert!(near(w.units[u].facing, [1.0, 0.0]));
        assert!((w.units[u].health - 80.0).abs() < 1e-3);
        // And the truck can go and unpack again.
        w.apply(
            1,
            Command::Unpack {
                regiment: packed,
                at: [30.0, 60.0],
            },
        )
        .unwrap();
    }
}
