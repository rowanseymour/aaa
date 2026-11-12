//! The rules of the game, played out with no rendering so that it can run
//! headless.
//!
//! A *match* is one play of a map: a *match setup* (the map, its players and
//! their nations, teams and rules) once it's running. Each *player* is a side
//! in it, controlled by a person or an AI.

mod attach;
pub mod economy;
mod fog;
pub mod path;
mod research;
mod supply;
mod terrain;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::f32::consts::{FRAC_PI_4, PI, TAU};

use aaa_map::pathing::{CELL_SIZE, tile_at};
use aaa_map::{
    DeployPoints, Domain, Fighter, Id, Map, Obstacle, Packs, Pathing, Requires, Shape,
    TerrainEffect, Turret, Unpacks, Weapon, Zone,
};

pub use aaa_map::{Player, Relation, Team};
pub use attach::Hq;
pub use economy::{Balance, Change, Fallen, Ledger, Limit, Notice, Treasury};
pub use fog::{Fog, Seen};
pub use research::Lacking;
pub use supply::{Bombarder, Held, Supplying};

/// Seconds of play one [`World::tick`] advances.
pub const TICK: f32 = 0.1;

/// Ticks [`World::landings`] keeps each landing for.
pub const LANDINGS_KEPT: u64 = 50;

/// A shot's projectile, flying to where it lands.
#[derive(Debug, Clone)]
pub struct Projectile {
    /// What fired it, of which player, and the index of its weapon.
    pub from: Target,
    pub by: u32,
    pub weapon: usize,
    /// The weapon, which outlives its firer once fired.
    shot: Weapon,
    pub target: Target,
    /// Where it lands.
    pub at: [f32; 2],
    /// The tick it lands on.
    pub lands: u64,
    /// Whether it was fired as bombardment.
    bombard: bool,
}

/// A projectile that has landed.
#[derive(Debug, Clone, PartialEq)]
pub struct Landing {
    pub tick: u64,
    pub from: Target,
    pub weapon: usize,
    pub target: Target,
    pub at: [f32; 2],
}

/// The share of its health a building taken over by force is left with.
pub const CAPTURE_HEALTH: f32 = 0.3;

/// How near a moving unit comes to an enemy to run into it.
pub const CONTACT: f32 = 1.5;

/// How near where it was going a unit has to stop to have arrived, and how
/// near on a move that has to end right there, such as to unpack.
pub const ARRIVAL_REACH: f32 = 0.5;
pub const EXACT_REACH: f32 = 0.1;

/// How near a unit's destination is for a move that has to end right there
/// to go straight at it.
const STRAIGHT_REACH: f32 = 3.0;

/// How many times a unit finds a way to a destination before giving up on
/// stopping there.
const WAYS_TRIED: u8 = 3;

/// How near a leg's end a unit taking it up has to be to skip it.
const LEG_REACH: f32 = 0.1;

/// How far to the side of its leg a unit gets before making for it again.
const OFF_LINE: f32 = 1.0;

/// Ticks between a moving unit's looks at how its move is going, and
/// between those of one that's given up turning onto its leg. The game
/// spreads them over 1 to 1.3 seconds and 0.3 to 0.6.
const LOOK: u32 = 12;
const LOOK_SNAPPED: u32 = 5;

/// The share of a regiment's control range its pursued enemy has to come
/// within for it to engage.
pub const PURSUIT_REACH: f32 = 0.6;

/// How far, squared, the enemy a regiment pursues gets from where it's
/// headed, against how far, squared, it is from the regiment, before the
/// regiment turns after it: about 22.5° off course.
pub const PURSUIT_DRIFT: f32 = 0.14646;

/// How far what a regiment guards moves its post before the regiment
/// follows.
const POST_DRIFT: f32 = 1.0;

/// A match in progress.
pub struct World {
    pub sides: Sides,
    pub pathing: Pathing,
    /// The terrain under each tile, for what it does to those on it.
    pub grounds: aaa_map::Grounds,
    pub terrain_rules: aaa_map::TerrainRules,
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
    /// The scenery still standing that a building going up clears away,
    /// as [`aaa_map::Map::clearable`] has it.
    pub clearable: Vec<(u32, Obstacle)>,
    /// The ids of the scenery buildings have cleared away, in the order
    /// they went.
    pub cleared: Vec<u32>,
    /// Each template's own cost, production and upkeep, by template.
    pub economies: HashMap<Id, aaa_map::Economy>,
    pub economy: aaa_map::EconomyRules,
    /// How each template gives supply, for those that do, by template.
    pub supplies: HashMap<Id, aaa_map::Supply>,
    /// The settings supply runs by.
    pub supply_rules: aaa_map::SupplyRules,
    pub guard_rules: aaa_map::GuardRules,
    /// Seconds an enemy's damage keeps what it hurt under siege.
    pub siege_seconds: f32,
    /// What each property does to supply, for those that do anything to
    /// it, by its id.
    pub supply_effects: HashMap<Id, aaa_map::SupplyEffect>,
    /// What each template's things are, by template, for the technologies
    /// and operations that work only on some.
    pub kinds: HashMap<Id, aaa_map::Kind>,
    /// The technologies each side has, by player id.
    pub technologies: BTreeMap<u32, Vec<Id>>,
    /// The special operations that bear on supply, by id.
    pub operations: HashMap<Id, aaa_map::Operation>,
    /// What those operations drop, by template, each standing anywhere and
    /// owned by nobody.
    pub drops: HashMap<Id, aaa_map::Building>,
    /// The health each template's buildings lose a second from when
    /// they're put on the map, by template.
    pub decays: HashMap<Id, f32>,
    /// How each organization template stands with supply, by template.
    /// Regiments of those without it are never in or out of supply.
    pub logistics: HashMap<Id, aaa_map::Logistics>,
    /// The share of its health each template gets back a second in
    /// supply, by template.
    pub resupply_rates: HashMap<Id, f32>,
    /// The regiment templates each building template recruits, which are
    /// those that can attach to it as an HQ.
    pub recruit_lists: HashMap<Id, Vec<Id>>,
    /// What sides own that's neither a regiment nor a building but may
    /// still add to their economy, such as where they start: each owner
    /// and template.
    pub holdings: Vec<(u32, String)>,
    /// Each side's money, by player id.
    pub treasuries: BTreeMap<u32, Treasury>,
    /// What sides have been told since these were last taken.
    pub notices: Vec<Notice>,
    /// How far each template sees, by template.
    pub sights: HashMap<Id, aaa_map::Sight>,
    /// What each side sees of the map.
    pub fog: Fog,
    /// Ticks played.
    pub ticks: u64,
    /// Projectiles on their way.
    pub flying: Vec<Projectile>,
    /// Projectiles that landed in the last [`LANDINGS_KEPT`] ticks, oldest
    /// first, for what shows them to find.
    pub landings: Vec<Landing>,
    /// The templates whose regiments are numbered, by template.
    numbered: HashSet<Id>,
    /// The last number each side has given a numbered template, by side
    /// and template.
    numbers: HashMap<(u32, String), u32>,
    /// An id no object has yet.
    next_id: u32,
    /// An id no commission has yet.
    next_commission: u32,
    /// What the next [`World::chance`] is drawn from.
    rng: u64,
}

pub struct Regiment {
    /// Its object's id in the map.
    pub id: u32,
    pub template: String,
    /// The number its name is given, for one of a numbered template: see
    /// [`World::set_numbered`].
    pub number: Option<u32>,
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
    /// Where it's on its way to unpack, and the way the building will
    /// face there.
    pub unpack_at: Option<([f32; 2], [f32; 2])>,
    /// The tick it asks again for room to unpack, once refused it.
    unpack_again: Option<u64>,
    /// The building it unpacked into, by index into [`World::buildings`].
    pub unpacked_into: Option<usize>,
    /// For one just put out by a building, the building's rally point, to
    /// move on to once it's rolled out.
    pub rally: Option<[f32; 2]>,
    /// The place in `units` of its leader, whose position is the
    /// regiment's for supply.
    pub leader: Option<usize>,
    /// The HQ it's attached to, by its organization's object id.
    pub hq: Option<u32>,
    /// Whether it was in supply at its last look.
    pub in_supply: bool,
    /// The health built up towards replacing the member in each place,
    /// for those lost or yet to arrive.
    pub replacing: Vec<f32>,
    /// Whether it's carrying out an order to bombard, which makes its
    /// shots bombardment and keeps it from resupplying.
    pub bombarding: bool,
    /// What has bombarded it lately, which keeps it out of supply.
    pub bombarded: Vec<Bombarder>,
    /// The properties it has for a time, such as an operation's.
    pub properties: Vec<Held>,
    /// The guard it's keeping, until it's given another order.
    pub guard: Option<Guard>,
    /// The terrain it's on, as [`aaa_map::Grounds::kind`] takes it.
    pub terrain: u16,
    /// The properties that terrain gives it.
    pub terrain_properties: Vec<Id>,
}

/// A regiment's order to keep a post: it comes back to it after every
/// fight, and beside something goes after whoever is fighting that.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guard {
    /// What it guards, for one that isn't guarding a place.
    pub of: Option<Enemy>,
    /// Where it rests: the place, or a point beside what it guards.
    pub post: [f32; 2],
    /// The way it faces there: away from what it guards.
    pub facing: [f32; 2],
    step: Step,
    /// An intruder it found no way to, left alone until its next fight or
    /// its post moves.
    unreachable: Option<usize>,
}

/// How far a guard has got toward its post since it last fought.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Unsent,
    Sent,
    /// As near as it can get.
    Arrived,
}

impl Regiment {
    /// Whether it's resting at the post it guards, which has it stand in
    /// its combat formation, aggressive, with a longer control range.
    pub fn guarding(&self) -> bool {
        self.engaged.is_empty() && self.guard.is_some_and(|g| g.step == Step::Arrived)
    }

    /// Its zone of control's radius now.
    pub fn control(&self, rules: &aaa_map::GuardRules) -> f32 {
        match self.guarding() {
            true => self.control_range * rules.control,
            false => self.control_range,
        }
    }

    /// Index into `formations` of the one it's in: its combat formation
    /// while engaged or attacking.
    pub fn current_formation(&self) -> usize {
        let fighting =
            !self.engaged.is_empty() || matches!(self.order, Order::Attack | Order::Pursue(_));
        match fighting || self.guarding() {
            true => self.combat.unwrap_or(self.formation),
            false => self.formation,
        }
    }

    /// Whether it takes on enemies that come within its control range or
    /// that it runs into, not only what fires on it: always while engaged,
    /// attacking or guarding, never while running, else as its formation
    /// is.
    pub fn aggressive(&self) -> bool {
        match self.order {
            Order::Run => false,
            Order::Attack | Order::Pursue(_) => true,
            Order::Move => {
                !self.engaged.is_empty()
                    || self.guarding()
                    || self.formations[self.formation].aggressive
            }
        }
    }
}

/// What a regiment can engage, or guard.
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

/// The leg of its path a unit is driving, to its next waypoint.
#[derive(Debug, Clone, Copy)]
struct Leg {
    /// The way along it, which the unit turns to and holds.
    heading: [f32; 2],
    /// Where it ends: the waypoint, less the room to turn the corner there.
    aim: [f32; 2],
    /// Whether the unit's given up turning onto it, and faces along it.
    snapped: bool,
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
    /// Where the leg to the next waypoint starts.
    from: [f32; 2],
    /// The leg it's driving, once it's taken it up.
    leg: Option<Leg>,
    /// Where it's to stop, until it has or has given up.
    goal: Option<[f32; 2]>,
    /// Whether it has to stop within [`EXACT_REACH`] of that.
    exact: bool,
    /// How many ways there it's taken.
    ways: u8,
    /// Ticks until it next looks at how its move is going.
    look: u32,
    /// World units a second it's going at.
    pub speed: f32,
    /// World units a second it gains each second; 0 for at once.
    acceleration: f32,
    /// Radians a second it turns; 0 for at once.
    turn_rate: f32,
    /// The radius of the circle it turns at its own top speed; 0 for one
    /// that turns at once.
    turn_radius: f32,
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
    /// While it's under siege, the tick that may end on.
    pub besieged: Option<u64>,
    /// The tick it left the map, having unpacked into a building.
    pub unpacked: Option<u64>,
    /// Whether it's yet to join its regiment, as a recruit's members are
    /// until supply brings them.
    pub awaited: bool,
    /// The terrain it's on, as [`aaa_map::Grounds::kind`] takes it: its
    /// regiment's, or its own tile's while it straggles.
    pub terrain: u16,
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

    /// Stands it straight behind its unit.
    fn behind(&mut self, position: [f32; 2], facing: [f32; 2]) {
        let hitch = aaa_map::from_local(position, facing, self.hitch);
        let [x, y] = self.coupling;
        self.position = aaa_map::from_local(hitch, facing, [-x, -y]);
        self.previous = self.position;
        self.facing = facing;
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
        self.died.is_none() && self.unpacked.is_none() && !self.awaited
    }

    fn die(&mut self, tick: u64) {
        self.health = 0.0;
        self.died = Some(tick);
        self.besieged = None;
        self.halt();
        self.speed = 0.0;
        self.target = None;
    }

    /// Sets it off along `path`, next waypoint last, to stop at its end.
    fn take(&mut self, path: Vec<[f32; 2]>, exact: bool) {
        self.goal = path.first().copied();
        self.path = path;
        self.from = self.position;
        self.leg = None;
        self.exact = exact;
        self.ways = 1;
        self.look = LOOK;
    }

    /// Stops it where it is, with nowhere left to go.
    fn halt(&mut self) {
        self.path.clear();
        self.leg = None;
        self.goal = None;
    }

    /// The middle of the tightest circle it can turn to its left, for a
    /// positive `side`, or its right.
    fn turning_circle(&self, side: f32) -> [f32; 2] {
        let [x, y] = self.position;
        let [fx, fy] = self.facing;
        let r = self.turn_radius.copysign(side);
        [x - fy * r, y + fx * r]
    }

    /// Whether `at` is somewhere it can't turn onto: inside the tightest
    /// circle it can turn either way.
    fn too_tight(&self, at: [f32; 2]) -> bool {
        let r = self.turn_radius;
        r > 0.0
            && [1.0, -1.0]
                .iter()
                .any(|&side| within_strictly(self.turning_circle(side), at, r))
    }

    /// Takes up the leg to its next waypoint, passing over the waypoints
    /// it's already level with or past. The leg ends short of a corner by
    /// the room it takes to turn it.
    fn take_leg(&mut self) {
        while self.leg.is_none() {
            let Some(&next) = self.path.last() else {
                return;
            };
            let along = [next[0] - self.from[0], next[1] - self.from[1]];
            let heading = normalised(along).unwrap_or(self.facing);
            let onward = (self.path.len().checked_sub(2))
                .and_then(|i| normalised([self.path[i][0] - next[0], self.path[i][1] - next[1]]));
            let lead = match onward {
                Some(onward) if self.turn_radius > 0.0 => {
                    let cos = heading[0] * onward[0] + heading[1] * onward[1];
                    let sin = (heading[0] * onward[1] - heading[1] * onward[0]).abs();
                    // The tangent of half the angle between the two legs.
                    let lead = self.turn_radius * sin / (1.0 + cos);
                    let length = libm::hypotf(along[0], along[1]);
                    if lead < length { lead } else { length }
                }
                _ => 0.0,
            };
            let aim = [next[0] - heading[0] * lead, next[1] - heading[1] * lead];
            let left =
                (aim[0] - self.position[0]) * heading[0] + (aim[1] - self.position[1]) * heading[1];
            if left <= 0.0 || within(self.position, aim, LEG_REACH) {
                self.path.pop();
                self.from = next;
                continue;
            }
            self.leg = Some(Leg {
                heading,
                aim,
                snapped: self.path.len() == 1 && self.too_tight(next),
            });
        }
    }

    /// Plays out a tick of heading along its path at up to `top` speed: it
    /// turns toward its leg's heading as fast as it can, speeds up toward
    /// `top` and goes the way it faces, until it's level with the leg's
    /// end, however far to the side that leaves it. It never slows to turn.
    fn drive(&mut self, top: f32) {
        self.take_leg();
        let (Some(&next), Some(leg)) = (self.path.last(), self.leg) else {
            self.speed = 0.0;
            return;
        };
        match leg.snapped || self.turn_rate <= 0.0 {
            true => self.facing = leg.heading,
            false => {
                steer(&mut self.facing, leg.heading, self.turn_rate * TICK);
            }
        }
        self.speed = match self.speed < top && self.acceleration > 0.0 {
            true => (self.speed + self.acceleration * TICK).min(top),
            false => top,
        };
        let step = self.speed * TICK;
        let left = (leg.aim[0] - self.position[0]) * leg.heading[0]
            + (leg.aim[1] - self.position[1]) * leg.heading[1];
        let ends = left <= step;
        // Only one going straight along the leg stops level with its end.
        let go = match ends && self.facing == leg.heading {
            true => left.max(0.0),
            false => step,
        };
        self.position[0] += self.facing[0] * go;
        self.position[1] += self.facing[1] * go;
        if ends {
            self.path.pop();
            self.from = next;
            self.leg = None;
            if self.path.is_empty() {
                self.speed = 0.0;
            }
            return;
        }
        self.look = self.look.saturating_sub(1);
        if self.look == 0 {
            self.think(next, leg);
        }
    }

    /// Looks at how the leg to `next` is going: gives up turning onto a
    /// last leg that ends somewhere it can't turn onto, and, once it's done
    /// turning but is left well to the side of its leg, makes for the leg
    /// again at an angle, or straight for the leg's end where that comes
    /// first.
    fn think(&mut self, next: [f32; 2], leg: Leg) {
        self.look = match leg.snapped {
            true => LOOK_SNAPPED,
            false => LOOK,
        };
        if self.turn_radius <= 0.0 {
            return;
        }
        if self.path.len() == 1 && !leg.snapped && self.too_tight(next) {
            self.facing = leg.heading;
            self.leg = Some(Leg {
                snapped: true,
                ..leg
            });
            self.look = LOOK_SNAPPED;
            return;
        }
        if self.facing != leg.heading {
            return;
        }
        let [hx, hy] = leg.heading;
        let back = [self.position[0] - next[0], self.position[1] - next[1]];
        // How far to the leg's left it is, and short of its end.
        let aside = hx * back[1] - hy * back[0];
        let short = -(hx * back[0] + hy * back[1]);
        if aside.abs() < OFF_LINE {
            return;
        }
        // At 45°, or as much less as turning in and back out again leaves
        // room for.
        let room = 1.0 - aside.abs() / (2.0 * self.turn_radius);
        let angle = libm::acosf(room.max(-1.0)).min(FRAC_PI_4);
        let along = aside.abs() / libm::tanf(angle);
        self.from = self.position;
        self.leg = None;
        self.look = LOOK;
        if along >= short {
            let to = [next[0] - self.position[0], next[1] - self.position[1]];
            self.facing = normalised(to).unwrap_or(self.facing);
            return;
        }
        self.path.push([
            self.position[0] + hy * aside + hx * along,
            self.position[1] - hx * aside + hy * along,
        ]);
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
    /// The number of the organization it stands in, as [`Regiment::number`],
    /// for one of a numbered template.
    pub number: Option<u32>,
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
    /// For one that defends itself, what it's noticed, which is all it
    /// fires on. Never forgotten, so not a sign of a fight.
    pub noticed: Vec<Enemy>,
    /// While it's under siege, the tick that may end on.
    pub besieged: Option<u64>,
    /// Where each of its fighter's turrets points.
    pub turrets: Vec<TurretAim>,
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
    /// Why its queue is held up, once its owner's been told.
    pub paused: Option<Pause>,
    /// Where what it commissions appears, then goes.
    pub deploy: Option<DeployPoints>,
    /// The organization the map has it standing in, without its members:
    /// packing puts its truck in the building's place there.
    pub organization: Option<Box<aaa_map::Regiment>>,
    /// Where what it commissions moves on to once out, if set.
    pub rally: Option<[f32; 2]>,
    /// The tick it last changed hands on.
    pub captured: Option<u64>,
    /// For one unpacked during the match, the regiment it unpacked from,
    /// by index into [`World::regiments`], which packing brings back.
    pub unpacked_from: Option<usize>,
    /// For a city's centre, how far around it the city reaches.
    pub zone: Option<f32>,
    /// For a city's centre, the ids of the buildings that change hands
    /// with it, as the map lists them: looked up as it changes hands, so
    /// one that names nothing then is passed over.
    pub members: Vec<u32>,
    /// The ground it keeps movers off until it's destroyed.
    pub obstacle: Option<Obstacle>,
    pub supply: Supplying,
    /// The properties it has for a time, such as a saboteur's.
    pub properties: Vec<Held>,
    /// The tick it was put on the map.
    pub created: u64,
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
    /// The money taken for it, given back if it's called off.
    pub paid: f32,
    /// Whether it's taken its share of its owner's limits, which it does
    /// on reaching the front of the queue once there's room.
    pub held: bool,
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
            paid: 0.0,
            held: false,
            id: 0,
        }
    }
}

/// Why a building's queue is held up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    /// Its owner hasn't room under a limit, such as `unit_limit_consumed`,
    /// for the first of the queue.
    Limit(&'static str),
    Disabled,
    /// It passes supply on and has none.
    Supply,
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
    /// Its owner lacks buildings or technologies that what it would raise
    /// needs.
    Lacking,
    /// Its owner hasn't the money for it.
    Unaffordable,
    /// The HQ that would raise it has no place left for it.
    Full,
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
    /// Posts a regiment at `to`, which it goes to as on [`Command::Move`]
    /// and comes back to after every fight, until it's given another
    /// order. Resting there it's as [`Regiment::guarding`] has it.
    Guard { regiment: usize, to: [f32; 2] },
    /// As [`Command::Guard`], but posted beside `thing`, following it and
    /// going after the enemy regiments fighting it, until it's gone.
    /// Refused for the regiment itself, or what's already gone.
    GuardThing { regiment: usize, thing: Enemy },
    /// Halts a regiment, its members forming up around where they stand on
    /// average, and drops where it was headed, so that once done with any
    /// engagement it stays put.
    Stop { regiment: usize },
    /// Sends a packed building to unpack at [`unpack_site`] for `at`, the
    /// building facing `facing`. Refused where it can't unpack or the
    /// building wouldn't fit.
    Unpack {
        regiment: usize,
        at: [f32; 2],
        facing: [f32; 2],
    },
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
    /// Attaches a regiment to an HQ with a place for its kind: the nearest
    /// for one that's detached, else the next after its own, round to the
    /// first again. Refused with none to go to.
    Attach { regiment: usize },
    /// Detaches a regiment from its HQ.
    Detach { regiment: usize },
}

/// Where a packed building sent to unpack at `at` stands: the nearest
/// whole units to it.
pub fn unpack_site(at: [f32; 2]) -> [f32; 2] {
    at.map(f32::round)
}

/// The way a building ordered to unpack facing `facing` faces, as a unit
/// vector.
fn unpack_facing(facing: [f32; 2]) -> [f32; 2] {
    normalised(facing).unwrap_or(aaa_map::UNPACKED_FACING)
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

    /// The yaw its turret `turret` would point at `at` with, from where the
    /// turret stands; `None` from right there.
    fn yaw_to(&self, turret: &Turret, at: [f32; 2]) -> Option<f32> {
        let [ahead, left] = turret.offset;
        let spot = aaa_map::from_local(self.position, self.facing, [-left, ahead]);
        let [right, ahead] = aaa_map::to_local(spot, self.facing, at);
        (right != 0.0 || ahead != 0.0).then(|| libm::atan2f(-right, ahead))
    }

    /// Whether its turret `turret` can be aimed at `at`.
    fn in_arc(&self, turret: &Turret, at: [f32; 2]) -> bool {
        turret.sweep >= TAU
            || self
                .yaw_to(turret, at)
                .is_none_or(|yaw| wrap(yaw - turret.start).abs() <= turret.sweep / 2.0)
    }
}

/// Where one of a building's turrets points, in radians from the
/// building's facing, counter-clockwise seen from above.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurretAim {
    pub yaw: f32,
    /// Seconds since it last turned toward something to fire on.
    idle: f32,
}

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

/// An angle in radians brought into -π to π.
fn wrap(angle: f32) -> f32 {
    angle - TAU * ((angle + PI) / TAU).floor()
}

/// Something a weapon can fire on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        world.grounds = map.grounds.clone();
        world.scenery = map.scenery.clone();
        world.clearable = map.clearable.clone();
        let regiments = map.regiments.iter().map(|r| r.id);
        let taken: HashSet<u32> = regiments
            .chain(map.buildings.iter().map(|b| b.id))
            .collect();
        world.holdings = map
            .things
            .iter()
            .filter(|t| !taken.contains(&t.id))
            .filter_map(|t| Some((aaa_map::owner(t)?, t.template.clone())))
            .collect();
        for p in &map.players {
            world.treasuries.insert(
                p.id,
                Treasury {
                    stockpile: p.stockpile,
                    shortage: None,
                },
            );
        }
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
        // Nothing's hidden until the match sets its fog.
        let fog = Fog::new(pathing.side, aaa_map::FogSetting::None);
        let mut world = World {
            sides,
            pathing,
            grounds: aaa_map::Grounds::default(),
            terrain_rules: aaa_map::TerrainRules::default(),
            regiments: Vec::new(),
            units: Vec::new(),
            buildings: Vec::new(),
            scenery: Vec::new(),
            clearable: Vec::new(),
            cleared: Vec::new(),
            ticks: 0,
            next_id: 1,
            next_commission: 0,
            flying: Vec::new(),
            landings: Vec::new(),
            packing: HashMap::new(),
            requires: HashMap::new(),
            economies: HashMap::new(),
            economy: aaa_map::EconomyRules::default(),
            supplies: HashMap::new(),
            supply_rules: aaa_map::SupplyRules::default(),
            guard_rules: aaa_map::GuardRules::default(),
            siege_seconds: 7.0,
            supply_effects: HashMap::new(),
            kinds: HashMap::new(),
            technologies: BTreeMap::new(),
            operations: HashMap::new(),
            drops: HashMap::new(),
            decays: HashMap::new(),
            logistics: HashMap::new(),
            resupply_rates: HashMap::new(),
            recruit_lists: HashMap::new(),
            rng: 0x9e37_79b9_7f4a_7c15,
            holdings: Vec::new(),
            treasuries: BTreeMap::new(),
            notices: Vec::new(),
            sights: HashMap::new(),
            fog,
            numbered: HashSet::new(),
            numbers: HashMap::new(),
        };
        for regiment in regiments {
            world.add_regiment(regiment);
        }
        world
    }

    /// Adds a regiment as the map has it, standing where its members do,
    /// returning its index.
    pub fn add_regiment(&mut self, regiment: &aaa_map::Regiment) -> usize {
        self.push_regiment(regiment, true)
    }

    /// Adds a regiment, numbering it if `numbered`.
    fn push_regiment(&mut self, regiment: &aaa_map::Regiment, numbered: bool) -> usize {
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
                health: fighter.as_ref().map_or(0.0, |f| f.health * m.health),
                reloads: fighter
                    .as_ref()
                    .map_or(Vec::new(), |f| vec![0.0; f.weapons.len()]),
                fighter,
                target: None,
                shot: None,
                died: None,
                besieged: None,
                unpacked: None,
                // One its regiment has lost comes back as a recruit's do.
                awaited: m.lost.is_some(),
                terrain: 0,
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
                from: [x, y],
                leg: None,
                goal: None,
                exact: false,
                ways: 0,
                look: 0,
                turn_radius: match m.turn_rate > 0.0 {
                    true => m.speed / m.turn_rate,
                    false => 0.0,
                },
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
        let number = match numbered {
            true => self.next_number(regiment.owner, &regiment.template),
            false => None,
        };
        self.regiments.push(Regiment {
            id: regiment.id,
            template: regiment.template.clone(),
            number,
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
            unpack_again: None,
            unpacked_into: None,
            rally: None,
            leader: regiment.primary,
            hq: regiment.hq,
            // Until its first look, so it doesn't start out marked.
            in_supply: true,
            replacing: (regiment.members.iter())
                .map(|m| m.lost.unwrap_or(0.0))
                .collect(),
            bombarding: false,
            bombarded: Vec::new(),
            properties: Vec::new(),
            guard: None,
            terrain: 0,
            terrain_properties: Vec::new(),
        });
        let ids = regiment.members.iter().map(|m| m.thing.id);
        self.next_id = ids
            .chain([regiment.id])
            .fold(self.next_id, |n, id| n.max(id + 1));
        r
    }

    /// Which templates' regiments are numbered, numbering those already
    /// here in the order they were added: regiments, then the buildings
    /// standing in an organization. Each side counts each template on its
    /// own, from 1, and never gives a number twice.
    pub fn set_numbered(&mut self, templates: HashSet<Id>) {
        self.numbered = templates;
        for r in 0..self.regiments.len() {
            if self.regiments[r].number.is_none() {
                let (owner, template) =
                    (self.regiments[r].owner, self.regiments[r].template.clone());
                self.regiments[r].number = self.next_number(owner, &template);
            }
        }
        for b in 0..self.buildings.len() {
            if self.buildings[b].number.is_none() {
                self.buildings[b].number = self.building_number(b);
            }
        }
    }

    /// The next number `owner` gives a regiment of `template`, if it's
    /// numbered and a side's.
    fn next_number(&mut self, owner: u32, template: &str) -> Option<u32> {
        if !self.numbered.contains(&Id::from(template)) || self.sides.player(owner).is_none() {
            return None;
        }
        let n = self
            .numbers
            .entry((owner, template.to_string()))
            .or_insert(0);
        *n += 1;
        Some(*n)
    }

    /// The next number for the organization the building at `b` stands in.
    fn building_number(&mut self, b: usize) -> Option<u32> {
        let building = &self.buildings[b];
        let template = building.organization.as_ref()?.template.clone();
        self.next_number(building.owner, &template)
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
        self.push_buildings(buildings, true);
    }

    /// Adds buildings, numbering them if `numbered`.
    fn push_buildings(&mut self, buildings: &[aaa_map::Building], numbered: bool) {
        for b in buildings {
            let health = b.fighter.health * b.health;
            self.buildings.push(Building {
                id: b.id,
                template: b.template.clone(),
                number: None,
                owner: b.owner,
                position: b.position,
                facing: b.facing,
                reloads: vec![0.0; b.fighter.weapons.len()],
                turrets: (b.fighter.turrets.iter())
                    .map(|t| TurretAim {
                        yaw: wrap(t.start),
                        idle: 0.0,
                    })
                    .collect(),
                fighter: b.fighter.clone(),
                health,
                target: None,
                noticed: Vec::new(),
                besieged: None,
                shot: None,
                state: match health > 0.0 {
                    true => State::Standing,
                    false => State::Disabled,
                },
                captured: None,
                unpacked_from: None,
                zone: b.zone,
                members: b.members.clone(),
                obstacle: b.obstacle.clone(),
                unpacking: None,
                packing: None,
                queue: VecDeque::new(),
                progress: 0,
                paused: None,
                deploy: b.deploy,
                organization: b.organization.clone(),
                rally: None,
                supply: Supplying::default(),
                properties: Vec::new(),
                created: self.ticks,
            });
            self.next_id = self.next_id.max(b.id + 1);
            if numbered {
                let at = self.buildings.len() - 1;
                self.buildings[at].number = self.building_number(at);
            }
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
                self.lost_member(u);
            }
        }
    }

    /// Tells of the regiment of the unit at `u`, just killed, if it has
    /// none left.
    fn lost_member(&mut self, u: usize) {
        let r = self.units[u].regiment;
        if self.destroyed(r) {
            self.notices.push(Notice::Destroyed {
                owner: self.regiments[r].owner,
                what: Fallen::Regiment(r),
            });
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
            | Command::Guard { regiment, .. }
            | Command::Attack { regiment, .. }
            | Command::Run { regiment, .. }
            | Command::Stop { regiment }
            | Command::Formation { regiment, .. } => Ok(self.may_command(player, regiment)?),
            Command::Pursue { regiment, enemy } => self.may_pursue(player, regiment, enemy),
            Command::GuardThing { regiment, thing } => self.may_guard(player, regiment, thing),
            Command::Unpack {
                regiment,
                at,
                facing,
            } => self.may_unpack(player, regiment, at, facing).map(drop),
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
            Command::Attach { regiment } => self.may_attach(player, regiment).map(drop),
            Command::Detach { regiment } => self.may_detach(player, regiment),
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
            Command::Guard { regiment, to } => self.order_guard(player, regiment, to)?,
            Command::GuardThing { regiment, thing } => {
                self.order_guard_thing(player, regiment, thing)?
            }
            Command::Unpack {
                regiment,
                at,
                facing,
            } => {
                self.order_unpack(player, regiment, at, facing)?;
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
            Command::Attach { regiment } => {
                let hq = self.may_attach(player, regiment)?;
                let left = self.regiments[regiment].hq.replace(hq);
                let left = left.map(|hq| Notice::Detached {
                    player,
                    hq,
                    regiment,
                });
                self.notices.extend(left);
                let joined = Notice::Attached {
                    player,
                    hq,
                    regiment,
                };
                self.notices.push(joined);
            }
            Command::Detach { regiment } => {
                self.may_detach(player, regiment)?;
                let left = self.regiments[regiment].hq.take();
                let left = left.map(|hq| Notice::Detached {
                    player,
                    hq,
                    regiment,
                });
                self.notices.extend(left);
            }
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
        if !self.armed(regiment)
            || enemy >= self.regiments.len()
            || self.destroyed(enemy)
            || !self.hostile(regiment, Enemy::Regiment(enemy))
        {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    /// Whether any of a regiment's members has something to fire.
    fn armed(&self, regiment: usize) -> bool {
        self.survivors(regiment).any(|u| {
            self.units[u]
                .fighter
                .as_ref()
                .is_some_and(|f| !f.weapons.is_empty())
        })
    }

    fn may_guard(&self, player: u32, regiment: usize, thing: Enemy) -> Result<(), Refused> {
        self.may_command(player, regiment)?;
        let there = match thing {
            Enemy::Regiment(g) => g != regiment && g < self.regiments.len(),
            Enemy::Building(b) => b < self.buildings.len(),
        };
        match there && !self.left(thing) {
            true => Ok(()),
            false => Err(Refused::Unable),
        }
    }

    /// Whether something a regiment could guard is no longer there to.
    fn left(&self, thing: Enemy) -> bool {
        match thing {
            Enemy::Regiment(g) => self.destroyed(g),
            Enemy::Building(b) => self.buildings[b].gone(),
        }
    }

    /// Where the regiment stops to unpack, relative to where it unpacks,
    /// if it can there.
    fn may_unpack(
        &self,
        player: u32,
        regiment: usize,
        at: [f32; 2],
        facing: [f32; 2],
    ) -> Result<[f32; 2], Refused> {
        self.may_command(player, regiment)?;
        let Some(unpacks) = &self.regiments[regiment].unpacks else {
            return Err(Refused::Unable);
        };
        if !self.room_to_unpack(unpacks, unpack_site(at), facing) {
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
        if !self.lacks(player, building, regiment).is_empty() {
            return Err(Refused::Lacking);
        }
        // What an HQ raises it takes on, queued or out.
        let hq = self.hq_of(building);
        let hq = hq.and_then(|id| self.hqs().into_iter().find(|h| h.id == id));
        if hq.is_some_and(|h| self.free_places(&h) == 0) {
            return Err(Refused::Full);
        }
        if self.cost(regiment) > self.stockpile(player) {
            return Err(Refused::Unaffordable);
        }
        Ok(())
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
        self.advance(player, regiment, target, Order::Move, None)
    }

    fn order_attack(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.advance(player, regiment, target, Order::Attack, None)
    }

    fn order_pursue(&mut self, player: u32, regiment: usize, enemy: usize) -> Result<(), Refused> {
        self.may_pursue(player, regiment, enemy)?;
        let r = &mut self.regiments[regiment];
        r.order = Order::Pursue(enemy);
        r.guard = None;
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
        unpack_at: Option<([f32; 2], [f32; 2])>,
    ) -> Result<(), NotYours> {
        self.may_command(player, regiment)?;
        let r = &mut self.regiments[regiment];
        r.order = order;
        r.guard = None;
        r.unpack_at = unpack_at;
        r.unpack_again = None;
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
        r.guard = None;
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
        r.guard = None;
        r.resume = None;
        r.unpack_at = None;
        r.rally = None;
        if r.engaged.is_empty() {
            let facing = r.facing;
            self.send(regiment, here, facing);
        }
        Ok(())
    }

    fn order_guard(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.may_command(player, regiment)?;
        let kept = self.regiments[regiment].guard;
        if kept.is_some_and(|g| g.of.is_none() && g.post == target) {
            return Ok(());
        }
        let from = self.regiments[regiment].position;
        let facing = normalised([target[0] - from[0], target[1] - from[1]]);
        self.post(regiment, None, target, facing);
        Ok(())
    }

    fn order_guard_thing(
        &mut self,
        player: u32,
        regiment: usize,
        thing: Enemy,
    ) -> Result<(), Refused> {
        self.may_guard(player, regiment, thing)?;
        if self.regiments[regiment].guard.and_then(|g| g.of) == Some(thing) {
            return Ok(());
        }
        let (post, facing) = self.beside(regiment, thing);
        self.post(regiment, Some(thing), post, Some(facing));
        Ok(())
    }

    /// Has a regiment guard `of` from `post`, setting off there unless
    /// it's engaged.
    fn post(
        &mut self,
        regiment: usize,
        of: Option<Enemy>,
        post: [f32; 2],
        facing: Option<[f32; 2]>,
    ) {
        let r = &mut self.regiments[regiment];
        let facing = facing.unwrap_or(r.facing);
        r.order = Order::Move;
        r.unpack_at = None;
        r.unpack_again = None;
        r.rally = None;
        r.guard = Some(Guard {
            of,
            post,
            facing,
            step: Step::Unsent,
            unreachable: None,
        });
        match r.engaged.is_empty() {
            true => {
                r.resume = None;
                self.go_to_post(regiment);
            }
            // Where it's headed once done, as for a move.
            false => r.resume = Some((post, facing)),
        }
    }

    /// The post of a regiment guarding `thing`, and the way it faces there:
    /// off the thing's edge by half the regiment's control range, on the
    /// side the regiment is, facing away.
    fn beside(&self, regiment: usize, thing: Enemy) -> ([f32; 2], [f32; 2]) {
        let r = &self.regiments[regiment];
        let here = self.centre(regiment);
        let there = self.middle(thing);
        let away = normalised([here[0] - there[0], here[1] - there[1]]).unwrap_or(r.facing);
        (self.post_beside(regiment, thing, away), away)
    }

    /// The point `away` from `thing` that a regiment guarding it rests at,
    /// kept on the map.
    fn post_beside(&self, regiment: usize, thing: Enemy, away: [f32; 2]) -> [f32; 2] {
        let radius = match thing {
            Enemy::Regiment(_) => 0.0,
            Enemy::Building(b) => {
                let shape = self.buildings[b].obstacle.as_ref().map(|o| o.shape);
                shape.map_or(0.0, bounding_radius)
            }
        };
        let off = radius + self.regiments[regiment].control_range / 2.0;
        let [x, y] = self.middle(thing);
        let size = self.pathing.side as f32 * CELL_SIZE;
        [
            (x + away[0] * off).clamp(0.0, size),
            (y + away[1] * off).clamp(0.0, size),
        ]
    }

    /// Where something a regiment can engage or guard is.
    fn middle(&self, thing: Enemy) -> [f32; 2] {
        match thing {
            Enemy::Regiment(e) => self.centre(e),
            Enemy::Building(b) => self.buildings[b].position,
        }
    }

    fn order_unpack(
        &mut self,
        player: u32,
        regiment: usize,
        at: [f32; 2],
        facing: [f32; 2],
    ) -> Result<[f32; 2], Refused> {
        let offset = self.may_unpack(player, regiment, at, facing)?;
        let at = unpack_site(at);
        let stop = [at[0] + offset[0], at[1] + offset[1]];
        let unpack_at = Some((at, unpack_facing(facing)));
        self.advance(player, regiment, stop, Order::Move, unpack_at)?;
        Ok(at)
    }

    /// Whether what `unpacks` makes would fit standing at `at` facing
    /// `facing`, with the room around it buildings need: among the
    /// buildings and scenery where it blocks, and anywhere on the map
    /// where it doesn't.
    pub fn room_to_unpack(&self, unpacks: &Unpacks, at: [f32; 2], facing: [f32; 2]) -> bool {
        let obstacle = unpacks.building.obstacle.as_ref();
        let placed = Obstacle {
            position: at,
            facing: unpack_facing(facing),
            shape: unpacks.room,
            soft: unpacks.room,
        };
        if obstacle.is_none() {
            return self.pathing.on_map(&placed);
        }
        let standing = self.buildings.iter().filter(|b| !b.gone());
        let others = standing.filter_map(|b| b.obstacle.as_ref());
        self.pathing
            .room_for(&placed, Domain::Land, others.chain(&self.scenery))
    }

    fn order_pack(&mut self, player: u32, building: usize) -> Result<(), Refused> {
        let ticks = self.may_pack(player, building)?;
        self.drop_queue(building);
        let b = &mut self.buildings[building];
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
        commission.paid = self.cost(&commission.regiment);
        commission.held = false;
        let left = self.stockpile(player) - commission.paid;
        self.set_stockpile(player, left);
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
        let paid = b.queue.remove(index).map_or(0.0, |c| c.paid);
        self.refund(player, paid);
        let b = &mut self.buildings[building];
        if index == 0 {
            b.progress = 0;
            b.paused = None;
        }
        Ok(())
    }

    /// Calls off all a building's commissions, giving its owner back what
    /// they paid.
    fn drop_queue(&mut self, building: usize) {
        let b = &mut self.buildings[building];
        let refund: f32 = b.queue.drain(..).map(|c| c.paid).sum();
        b.progress = 0;
        b.paused = None;
        let owner = b.owner;
        self.refund(owner, refund);
    }

    fn refund(&mut self, player: u32, amount: f32) {
        if amount > 0.0 {
            let back = self.stockpile(player) + amount;
            self.set_stockpile(player, back);
        }
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
        // A move to unpack has to end right there.
        let exact = r.unpack_at.is_some();
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
            self.take_place(u, place, facing, exact);
        }
    }

    /// Sends a unit to stand at `place` facing `facing`.
    pub(crate) fn take_place(&mut self, u: usize, place: [f32; 2], facing: [f32; 2], exact: bool) {
        let way = self.plan(u, place, exact);
        let unit = &mut self.units[u];
        unit.take(way, exact);
        unit.closing = false;
        unit.end_facing = facing;
        if unit.path.is_empty() {
            unit.goal = None;
            unit.facing = facing;
        }
    }

    /// A number from 0 up to 1, the same in every copy of a match.
    pub(crate) fn chance(&mut self) -> f32 {
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
    }

    /// The way a unit takes to stop at `to`, last waypoint first, `exact`
    /// for a move that has to end right there. It goes straight at what's
    /// near enough on such a move, and turns at once to what's ahead of it
    /// but nearer than it could turn onto; otherwise it finds a way.
    fn plan(&mut self, u: usize, to: [f32; 2], exact: bool) -> Vec<[f32; 2]> {
        let unit = &self.units[u];
        let off = [to[0] - unit.position[0], to[1] - unit.position[1]];
        if exact && within_strictly(unit.position, to, STRAIGHT_REACH) {
            return vec![to];
        }
        let [fx, fy] = unit.facing;
        let ahead = fx * off[0] + fy * off[1] >= 0.0;
        let side = fx * off[1] - fy * off[0];
        let r = unit.turn_radius;
        if r > 0.0 && ahead && within_strictly(unit.turning_circle(side), to, r) {
            if let Some(facing) = normalised(off) {
                self.units[u].facing = facing;
            }
            return vec![to];
        }
        self.way(u, to)
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
            soft: shape,
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
        self.stand();
        self.notice();
        let fired_on = self.fight();
        self.engage(&fired_on);
        self.keep_guard();
        self.close_in();
        self.occupy();
        self.decay();
        self.lift_sieges();
        self.supply();
        self.resupply();
        self.produce();
        self.rally();
        self.unpack();
        self.earn();
        self.look();
        self.ticks += 1;
    }

    /// Moves buildings on with what they're commissioning, and puts out
    /// what's done where they deploy it, sent on to where it goes.
    fn produce(&mut self) {
        for b in 0..self.buildings.len() {
            if self.held_up(b) {
                continue;
            }
            let building = &mut self.buildings[b];
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
            log::debug!(
                "{} {} put out by {} {}",
                regiment.template,
                regiment.id,
                self.buildings[b].template,
                self.buildings[b].id
            );
            self.regiments[r].hq = self.hq_of(b);
            self.notices.push(Notice::PutOut {
                player: owner,
                building: b,
                regiment: r,
            });
            // Those that resupply come out as their leader alone, the
            // rest to follow.
            let used = self.logistics_of(&regiment.template);
            if used.is_some_and(|l| l.supply_used) {
                let leader = self.regiments[r].leader.unwrap_or(0);
                for (p, u) in self.regiments[r].units.clone().into_iter().enumerate() {
                    // Only those resupply can bring.
                    let template = Id::from(self.units[u].template.as_str());
                    let comes = self.resupply_rates.get(&template).is_some_and(|&r| r > 0.0);
                    self.units[u].awaited = p != leader && comes;
                }
            }
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
            if unit.awaited {
                continue;
            }
            unit.take(vec![aaa_map::from_local(to, facing, offset)], false);
            unit.end_facing = facing;
        }
    }

    /// Unpacks the packed buildings that have reached where they were sent
    /// to unpack, each leaving the map for the building it makes, and
    /// calls it off for those that can't get there or no longer have room.
    fn unpack(&mut self) {
        for r in 0..self.regiments.len() {
            let Some((at, facing)) = self.regiments[r].unpack_at else {
                continue;
            };
            if self.destroyed(r) {
                self.regiments[r].unpack_at = None;
                continue;
            }
            let again = self.regiments[r].unpack_again;
            if self.underway(r) || again.is_some_and(|tick| self.ticks < tick) {
                continue;
            }
            // The truck is the regiment's one survivor: the regiment
            // isn't destroyed and holds a single packed building.
            let (Some(unpacks), Some(truck)) =
                (self.regiments[r].unpacks.clone(), self.survivors(r).next())
            else {
                self.regiments[r].unpack_at = None;
                continue;
            };
            // It unpacks where it's stopped, if that's where it was sent.
            let stop = [at[0] + unpacks.offset[0], at[1] + unpacks.offset[1]];
            let here = self.units[truck].position;
            let at = unpack_site([here[0] - unpacks.offset[0], here[1] - unpacks.offset[1]]);
            let there = within(here, stop, ARRIVAL_REACH);
            if there && !self.room_to_unpack(&unpacks, at, facing) && again.is_none() {
                self.regiments[r].unpack_again = Some(self.ticks + u64::from(LOOK));
                continue;
            }
            let regiment = &mut self.regiments[r];
            regiment.unpack_at = None;
            regiment.unpack_again = None;
            if !there || !self.room_to_unpack(&unpacks, at, facing) {
                let player = self.regiments[r].owner;
                let told = Notice::CannotUnpack {
                    player,
                    regiment: r,
                };
                self.notices.push(told);
                continue;
            }
            let truck_id = self.units[truck].id;
            // It keeps the share of its health it had as a truck.
            let health = self
                .survivors(r)
                .filter_map(|u| Some(self.units[u].health / self.units[u].fighter.as_ref()?.health))
                .fold(1.0_f32, f32::min);
            for u in self.regiments[r].units.clone() {
                let unit = &mut self.units[u];
                if unit.alive() {
                    unit.unpacked = Some(self.ticks);
                    unit.halt();
                    unit.target = None;
                }
            }
            let mut building = unpacks.building;
            // The truck and its building are one thing to the player, so
            // it keeps its id and its place in the truck's organization.
            building.id = truck_id;
            building.owner = self.regiments[r].owner;
            building.position = at;
            building.facing = facing;
            building.health = health;
            if let Some(o) = &mut building.obstacle {
                o.position = at;
                o.facing = facing;
                self.pathing.block(o);
                self.clear_under(o);
            }
            let b = self.buildings.len();
            self.regiments[r].unpacked_into = Some(b);
            // It keeps the truck's number rather than taking one.
            self.push_buildings(std::slice::from_ref(&building), false);
            self.buildings[b].number = self.regiments[r].number;
            log::debug!(
                "{} {} unpacking into {}",
                self.regiments[r].template,
                truck_id,
                building.template
            );
            self.buildings[b].unpacked_from = Some(r);
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

    /// Takes away for good the scenery whose shape overlaps `footprint`, a
    /// building's as it goes up.
    fn clear_under(&mut self, footprint: &Obstacle) {
        let cleared = &mut self.cleared;
        self.clearable.retain(|(id, shape)| {
            let under = shape.overlaps(footprint);
            if under {
                cleared.push(*id);
            }
            !under
        });
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
        log::debug!("{} {} packed up", building.template, building.id);
        let share = building.health / building.fighter.health;
        let at = building.position;
        let owner = building.owner;
        if let Some(o) = building.obstacle.clone() {
            self.pathing.unblock(&o);
        }
        let (id, organization) = (building.id, building.organization.clone());
        let from = building.unpacked_from;
        let stand = [at[0] + packs.offset[0], at[1] + packs.offset[1]];
        // A truck that unpacked here comes back, in its organization, to
        // whoever holds the building now.
        let r = match from {
            Some(r) => {
                self.return_truck(r, stand, packs.facing);
                self.regiments[r].owner = owner;
                r
            }
            None => {
                // It hands the truck its id.
                let mut truck = packs.truck;
                match truck.lone() {
                    true => truck.renumber(id),
                    // A truck that's an organization takes the id itself,
                    // its members new ones.
                    false => {
                        let first = self.take_ids(truck.members.len() as u32);
                        for (m, id) in truck.members.iter_mut().zip(first..) {
                            m.thing.id = id;
                        }
                        truck.id = id;
                    }
                }
                if let Some(mut within) = organization.map(|o| *o) {
                    within.members = std::mem::take(&mut truck.members);
                    within.unpacks = truck.unpacks.take();
                    truck = within;
                }
                truck.owner = owner;
                truck.place(stand, packs.facing);
                // It keeps the building's number rather than taking one.
                let r = self.push_regiment(&truck, false);
                self.regiments[r].number = self.buildings[b].number;
                r
            }
        };
        for u in self.regiments[r].units.clone() {
            let unit = &mut self.units[u];
            if unit.alive()
                && let Some(f) = &unit.fighter
            {
                unit.health = f.health * share;
            }
        }
        self.buildings[b].state = State::Packed(self.ticks, r);
    }

    /// Puts the members of a regiment that unpacked back on the map, in
    /// its formation around `position` facing `facing`, as if just made:
    /// it keeps only its ids and place.
    fn return_truck(&mut self, regiment: usize, position: [f32; 2], facing: [f32; 2]) {
        let r = &mut self.regiments[regiment];
        r.unpacked_into = None;
        r.unpack_at = None;
        r.rally = None;
        r.order = Order::Move;
        r.engaged.clear();
        r.resume = None;
        r.position = position;
        r.facing = facing;
        let offsets = &r.formations[r.current_formation()].offsets;
        for (&u, &offset) in r.units.iter().zip(offsets) {
            let unit = &mut self.units[u];
            if unit.unpacked.take().is_none() {
                continue;
            }
            let at = aaa_map::from_local(position, facing, offset);
            unit.position = at;
            unit.previous = at;
            unit.facing = facing;
            unit.end_facing = facing;
            unit.halt();
            unit.speed = 0.0;
            unit.closing = false;
            unit.target = None;
            unit.shot = None;
            unit.reloads.fill(0.0);
            if let Some(t) = &mut unit.trailer {
                t.behind(at, facing);
            }
        }
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
            if within_strictly(self.centre(r), self.position(from), reach) {
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
            // What's already gone or out of reach would only be dropped
            // again, such as what fired the tick it died.
            if regiment.order == Order::Run
                || regiment.engaged.contains(&enemy)
                || !self.within_range(r, enemy)
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
                    unit.halt();
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
            unit.take(way, false);
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
                .any(|&t| within_strictly(here, self.position(t), regiment.operational_range));
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
        if let Some(guard) = &mut regiment.guard {
            guard.step = Step::Unsent;
        }
        if regiment.engaged.is_empty() {
            let facing = regiment.facing;
            self.send(r, here, facing);
        }
    }

    /// Has each regiment keeping a guard that isn't engaged go after the
    /// nearest intruder, or else to its post, which follows what it
    /// guards; the guard ends once that's gone.
    fn keep_guard(&mut self) {
        // Who's engaged with what, looked up once for all that guard things.
        let escorts = |r: &Regiment| r.guard.is_some_and(|g| g.of.is_some());
        let engaging: Vec<(usize, Enemy)> = match self.regiments.iter().any(escorts) {
            true => (self.regiments.iter().enumerate())
                .flat_map(|(e, r)| r.engaged.iter().map(move |&with| (e, with)))
                .collect(),
            false => Vec::new(),
        };
        for r in 0..self.regiments.len() {
            let Some(guard) = self.regiments[r].guard else {
                continue;
            };
            if self.destroyed(r) {
                continue;
            }
            if guard.of.is_some_and(|of| self.left(of)) {
                self.regiments[r].guard = None;
                self.give_up(r);
                continue;
            }
            if !self.regiments[r].engaged.is_empty() {
                continue;
            }
            let after = match self.regiments[r].order {
                Order::Pursue(e) => Some(e),
                _ => None,
            };
            let intruder = self.intruder(r, guard, after, &engaging);
            if let Some(e) = intruder {
                if after != Some(e) {
                    let regiment = &mut self.regiments[r];
                    regiment.order = Order::Pursue(e);
                    regiment.resume = None;
                    regiment.guard = Some(Guard {
                        step: Step::Unsent,
                        ..guard
                    });
                    self.chase(r, e, true);
                }
                continue;
            }
            if after.is_some() {
                self.give_up(r);
            }
            self.go_to_post(r);
        }
    }

    /// Sends a guard with nobody to fight to its post, if it's not been
    /// since it last fought or the post has moved, and has it rest once
    /// it's got as near as it can.
    fn go_to_post(&mut self, r: usize) {
        let Some(mut guard) = self.regiments[r].guard else {
            return;
        };
        let underway = self.underway(r);
        if let Some(of) = guard.of {
            // It keeps to the side it was on when it last set off.
            let (post, facing) = match guard.step {
                Step::Unsent => self.beside(r, of),
                _ => (self.post_beside(r, of, guard.facing), guard.facing),
            };
            // A post that's moved a little waits until the guard is there.
            let moved = match underway {
                true => !within(post, guard.post, POST_DRIFT),
                false => post != guard.post,
            };
            if guard.step == Step::Unsent {
                (guard.post, guard.facing) = (post, facing);
            } else if moved {
                guard = Guard {
                    post,
                    step: Step::Unsent,
                    unreachable: None,
                    ..guard
                };
            }
        }
        let before = self.regiments[r].current_formation();
        if guard.step == Step::Unsent {
            guard.step = Step::Sent;
            self.regiments[r].guard = Some(guard);
            self.send(r, guard.post, guard.facing);
        } else if guard.step == Step::Sent && !underway {
            guard.step = Step::Arrived;
            self.regiments[r].guard = Some(guard);
            if self.regiments[r].current_formation() != before {
                self.take_places(r);
            }
        }
    }

    /// The enemy regiment a guard goes after: the nearest of those
    /// `engaging` what it guards or, with none, the one that's engaged with
    /// first or fires on. The one it's `after` already counts as nearer, so
    /// the guard doesn't flit between two much the same way off.
    fn intruder(
        &self,
        r: usize,
        guard: Guard,
        after: Option<usize>,
        engaging: &[(usize, Enemy)],
    ) -> Option<usize> {
        let of = guard.of?;
        if !self.armed(r) {
            return None;
        }
        let fair = |e: usize| {
            guard.unreachable != Some(e)
                && !self.destroyed(e)
                && self.hostile(r, Enemy::Regiment(e))
        };
        let mut found: Vec<usize> = (engaging.iter())
            .filter(|&&(e, with)| with == of && fair(e))
            .map(|&(e, _)| e)
            .collect();
        if found.is_empty() {
            let fought = match of {
                Enemy::Regiment(g) => self.regiments[g].engaged.first().copied(),
                Enemy::Building(b) => self.buildings[b].target.map(|t| self.enemy(t)),
            };
            if let Some(Enemy::Regiment(e)) = fought {
                found.extend(fair(e).then_some(e));
            }
        }
        let here = self.centre(r);
        let far = |e: usize| {
            let bias = if after == Some(e) {
                self.guard_rules.bias
            } else {
                0.0
            };
            distance(here, self.centre(e)) - bias
        };
        found.into_iter().min_by(|&a, &b| far(a).total_cmp(&far(b)))
    }

    /// Moves a regiment that isn't engaged on after the enemy regiment `e`
    /// it's pursuing: engaging it once it's in [`PURSUIT_REACH`], else
    /// turning after it as it gets away, and giving up where it stops
    /// short of it.
    fn pursue(&mut self, r: usize, e: usize) {
        if self.destroyed(r) {
            return;
        }
        let reach = PURSUIT_REACH * self.regiments[r].control(&self.guard_rules);
        let zone = self.control_zone(r);
        let in_control = (self.survivors(e)).any(|u| self.in_zone(&zone, Target::Unit(u)));
        if in_control && within(self.centre(r), self.centre(e), reach) {
            let enemy = Enemy::Regiment(e);
            self.regiments[r].engaged.push(enemy);
            self.hold(r, enemy);
            return;
        }
        if !self.chase(r, e, false) && !self.underway(r) {
            if let Some(guard) = &mut self.regiments[r].guard {
                guard.unreachable = Some(e);
            }
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
        let zone = self.control_zone(r);
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
                && self.in_zone(&zone, t)
                && self.hostile(r, enemy)
                && fires_on(t)
            {
                found.push(enemy);
            }
        }
        found
    }

    /// The tiles within a regiment's control range of where it stands.
    fn control_zone(&self, r: usize) -> Zone {
        let reach = self.regiments[r].control(&self.guard_rules);
        self.pathing.zone(self.centre(r), reach)
    }

    /// Whether `t` is in `zone`: by the tile under it, or for what blocks
    /// tiles any of those under it.
    fn in_zone(&self, zone: &Zone, t: Target) -> bool {
        let blocking = match t {
            Target::Building(b) => self.buildings[b].obstacle.as_ref(),
            Target::Unit(_) => None,
        };
        match blocking {
            Some(o) => zone.meets(o),
            None => zone.holds(self.position(t)),
        }
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
                .any(|u| within_strictly(here, self.units[u].position, reach)),
            Enemy::Building(b) => {
                let building = &self.buildings[b];
                building.standing() && within_strictly(here, building.position, reach)
            }
        };
        near && self.hostile(r, enemy)
    }

    /// Stops a regiment that's just engaged where it stands, in its combat
    /// formation facing `enemy`, to carry on where it was going afterwards.
    fn hold(&mut self, r: usize, enemy: Enemy) {
        let here = self.centre(r);
        let there = self.middle(enemy);
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
        // A guard picks an intruder or its post afresh.
        if let Some(guard) = &mut self.regiments[r].guard {
            (guard.step, guard.unreachable) = (Step::Unsent, None);
            self.regiments[r].resume = None;
            return;
        }
        match self.regiments[r].resume.take() {
            Some((target, facing)) => self.send(r, target, facing),
            None => self.take_places(r),
        }
    }

    /// Moves every unit on along its path. One that's run out of path
    /// short of where it was to stop finds another way from where it is,
    /// until it's tried enough; once done it faces as it was told to.
    fn travel(&mut self) {
        for u in 0..self.units.len() {
            let limit = self.speed_limit(u).unwrap_or(f32::INFINITY);
            let unit = &mut self.units[u];
            unit.previous = unit.position;
            unit.drive(self.regiments[unit.regiment].speed.min(limit));
            if let (true, Some(goal)) = (unit.path.is_empty(), unit.goal) {
                let reach = match unit.exact {
                    true => EXACT_REACH,
                    false => ARRIVAL_REACH,
                };
                let (exact, ways) = (unit.exact, unit.ways);
                let again = match within(unit.position, goal, reach) || ways >= WAYS_TRIED {
                    true => Vec::new(),
                    false => self.plan(u, goal, exact),
                };
                let unit = &mut self.units[u];
                match again.is_empty() {
                    true => {
                        unit.goal = None;
                        unit.facing = unit.end_facing;
                    }
                    false => {
                        unit.take(again, exact);
                        (unit.goal, unit.ways) = (Some(goal), ways + 1);
                    }
                }
            }
            let unit = &mut self.units[u];
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
        let mut launched = Vec::new();
        // Buildings that defend themselves fire even while disabled.
        let disabled = (0..self.buildings.len())
            .filter(|&b| {
                let b = &self.buildings[b];
                b.state == State::Disabled && b.fighter.notice.is_some()
            })
            .map(Target::Building);
        let shooters: Vec<Target> = self.targets().chain(disabled).collect();
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
        let window = (self.siege_seconds / TICK).round() as u64;
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
                    let held = hit
                        .bombard
                        .map_or(0.0, |recharge| self.supply_rules.bombard_timeout + recharge);
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

    /// Deals with a building out of health: `by` captures it if it can be
    /// captured, else it's destroyed if it can be, else disabled.
    fn fall(&mut self, b: usize, by: Option<u32>) {
        let ticks = self.ticks;
        let building = &mut self.buildings[b];
        building.health = 0.0;
        building.target = None;
        if building.fighter.destructible {
            building.besieged = None;
        }
        // Cities change hands by who holds them instead.
        let taker = by.filter(|_| building.fighter.capturable && building.zone.is_none());
        if let Some(by) = taker {
            building.health = CAPTURE_HEALTH * building.fighter.health;
            self.change_hands(b, by);
        } else if building.fighter.destructible {
            building.state = State::Destroyed(ticks);
            if let Some(o) = building.obstacle.take() {
                self.pathing.unblock(&o);
            }
            self.notices.push(Notice::Destroyed {
                owner: building.owner,
                what: Fallen::Building(b),
            });
            self.drop_queue(b);
        } else {
            building.state = State::Disabled;
        }
    }

    /// Takes from each building what its own properties cost it, a second
    /// at a time from when it was put on the map: plain loss, whatever its
    /// defence.
    fn decay(&mut self) {
        for b in 0..self.buildings.len() {
            let building = &self.buildings[b];
            let age = self.ticks - building.created;
            let rate = self.decays.get(&Id::from(building.template.as_str()));
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
    fn lift_sieges(&mut self) {
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

    /// Gives a building, and the rest of its city if it's one's centre, to
    /// `owner`.
    fn change_hands(&mut self, b: usize, owner: u32) {
        let taken = &self.buildings[b];
        log::debug!("{} {} taken by player {owner}", taken.template, taken.id);
        // Each side's told only of what it sees, but a side always sees its
        // own: the loser's told while it's still theirs, the taker once it
        // is.
        self.notices.push(Notice::Lost {
            player: taken.owner,
            building: b,
            by: owner,
        });
        let ids = &self.buildings[b].members;
        let members: Vec<usize> = (0..self.buildings.len())
            .filter(|&i| i != b && ids.contains(&self.buildings[i].id))
            .collect();
        for i in std::iter::once(b).chain(members) {
            self.drop_queue(i);
            // An HQ that changes side loses its regiments.
            if let Some(hq) = self.hq_of(i) {
                self.detach_all(hq);
            }
            let building = &mut self.buildings[i];
            building.owner = owner;
            building.captured = Some(self.ticks);
            building.target = None;
            building.rally = None;
            // A relay has to connect on its new side.
            building.supply.judged = false;
        }
        self.notices.push(Notice::Captured {
            player: owner,
            building: b,
        });
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

    /// Has each building that defends itself notice what's come near enough
    /// for one of its weapons to aim at, for good: a unit's whole regiment.
    fn notice(&mut self) {
        for b in 0..self.buildings.len() {
            let building = &self.buildings[b];
            let Some(near) = building.fighter.notice.filter(|_| !building.gone()) else {
                continue;
            };
            let s = Target::Building(b);
            let zone = self.pathing.zone(building.position, near);
            let seen: Vec<Enemy> = self
                .targets()
                .filter(|&t| self.in_zone(&zone, t))
                .filter(|&t| {
                    let weapons = &building.fighter.weapons;
                    weapons
                        .iter()
                        .any(|w| self.can_target(s, w, t) && self.in_arc(b, w, t))
                })
                .map(|t| self.enemy(t))
                .collect();
            let noticed = &mut self.buildings[b].noticed;
            for e in seen {
                if !noticed.contains(&e) {
                    noticed.push(e);
                }
            }
        }
    }

    /// The regiment or building a target is, or is in.
    fn enemy(&self, t: Target) -> Enemy {
        match t {
            Target::Unit(u) => Enemy::Regiment(self.units[u].regiment),
            Target::Building(b) => Enemy::Building(b),
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
            noticed.contains(&self.enemy(t))
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

    fn can_hit(&self, s: Target, weapon: &Weapon, t: Target) -> bool {
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
    fn can_target(&self, s: Target, weapon: &Weapon, t: Target) -> bool {
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
    fn standing_flag(&self, by: u32, owner: u32) -> Id {
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

    /// The armed things in the zone of the city centred on `c`, each with
    /// its owner and its health's share. Independent sides' and nobody's
    /// don't count, nor does the centre.
    pub(crate) fn occupants(&self, c: usize, zone: f32) -> Vec<(u32, f32)> {
        let zone = self.pathing.zone(self.buildings[c].position, zone);
        let counts = |o: u32| self.sides.player(o).is_some_and(|p| !p.independent);
        self.targets()
            .filter(|&t| t != Target::Building(c) && self.in_zone(&zone, t))
            .filter(|&t| counts(self.owner(t)))
            .filter_map(|t| {
                let f = self.fighter(t).filter(|f| !f.weapons.is_empty())?;
                let health = match t {
                    Target::Unit(u) => self.units[u].health,
                    Target::Building(b) => self.buildings[b].health,
                };
                Some((self.owner(t), health / f.health))
            })
            .collect()
    }

    /// Who should hold the city centred on `c`: its owner while any of its
    /// own are in the zone or no enemy of its is, else the side there with
    /// none of its enemies there and no ally there stronger or in danger.
    fn holder(&self, c: usize, zone: f32) -> Option<u32> {
        let owner = self.buildings[c].owner;
        let occupants = self.occupants(c, zone);
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
fn damage(weapon: &Weapon, target: &Fighter, ground: &[&TerrainEffect], scale: f32) -> f32 {
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

/// Where a deployment offset from a building standing at `position`
/// facing `facing` lies.
fn deploy_point(position: [f32; 2], facing: [f32; 2], offset: [f32; 2]) -> [f32; 2] {
    aaa_map::from_local(position, facing, offset)
}

/// The radius of the circle round a shape: through a rectangle's corners.
fn bounding_radius(shape: Shape) -> f32 {
    match shape {
        Shape::Point => 0.0,
        Shape::Circle { radius } => radius,
        Shape::Rect { width, height } => libm::hypotf(width, height) / 2.0,
    }
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

/// Whether `a` and `b` are nearer than `reach` apart.
fn within_strictly(a: [f32; 2], b: [f32; 2], reach: f32) -> bool {
    squared_distance(a, b) < reach * reach
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
    use aaa_map::{Obstacle, Shape, SupplyEffect};
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
            lost: None,
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
            hq: None,
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
            hq: None,
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
    fn units_start_as_hurt_as_the_map_has_them() {
        let mut m = member(2, [10.0, 10.0], 2.0);
        m.fighter = Some(aaa_map::Fighter {
            health: 200.0,
            ..aaa_map::Fighter::default()
        });
        m.health = 0.25;
        let regiment = aaa_map::Regiment {
            hq: None,
            id: 2,
            template: "soldier".into(),
            owner: 7,
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
            unpacks: None,
        };
        let w = World::from_regiments(
            Sides::default(),
            Pathing::uniform(20, Domain::Land),
            &[regiment],
        );
        assert_eq!(w.units[0].health, 50.0);
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
        // Left to the side of where it was going, but near enough.
        assert!(!near(u.position, [20.0, 10.0]));
        assert!(
            within(u.position, [20.0, 10.0], ARRIVAL_REACH),
            "{:?}",
            u.position
        );
    }

    #[test]
    fn units_that_turn_over_time_stop_near_what_is_inside_their_turning_circle() {
        // It turns round a radius of 2 at its top speed.
        for to in [
            [10.0, 4.0],
            [11.0, 10.0],
            [8.0, 10.5],
            [10.0, 9.0],
            [4.0, 8.0],
        ] {
            let mut w = vehicle(0.0, 1.0, None);
            w.apply(7, Command::Move { regiment: 0, to }).unwrap();
            let mut ticks = 0;
            while w.units[0].moving() {
                w.tick();
                ticks += 1;
                assert!(ticks < 300, "{to:?}: {:?}", w.units[0].position);
            }
            let at = w.units[0].position;
            assert!(within(at, to, ARRIVAL_REACH), "{to:?}: {at:?}");
        }
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
            soft: Shape::Circle { radius: 4.0 },
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
            soft: Shape::Rect {
                width: 41.0,
                height: 2.0,
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
            hq: None,
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
        let lost = Notice::Destroyed {
            owner: w.regiments[0].owner,
            what: Fallen::Regiment(0),
        };
        assert_eq!(w.notices, [lost]);
    }

    fn rifle(range: f32, amount: f32) -> Weapon {
        Weapon {
            range,
            min_range: 0.0,
            recharge: 2.0,
            area: 0.0,
            max_targets: 0,
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
            turret_point: None,
            aimed_by: None,
            spawn_position: [0.0; 3],
            sounds: Vec::new(),
            effects: Default::default(),
            projectile: None,
            flight: None,
            bombard: false,
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
            attack_height: 1.45,
            notice: None,
            turrets: Vec::new(),
            soft: Shape::Point,
            target: Shape::Point,
            target_square: false,
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
                    hq: None,
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
        let side = |id, relations: &[u32]| Player {
            independent: false,
            ..player(id, None, relations, &[])
        };
        Sides {
            players: vec![
                side(1, &[0, 3, 1]),
                side(2, &[3, 0, 3]),
                side(3, &[1, 3, 0]),
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
    fn moving_regiments_carry_on_past_what_dies_as_it_fires_on_them() {
        // A gun that fires on the move and kills outright.
        let mut gun = rifle(12.0, 500.0);
        gun.turret = true;
        let mut w = battle(
            &[1, 2],
            vec![soldier(vec![gun]), soldier(vec![rifle(12.0, 10.0)])],
            at_war(),
        );
        w.units[1].position = [10.0, 30.0];
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [10.0, 60.0],
            },
        )
        .unwrap();
        // Each fires as the first comes into range, 4s on.
        let mut before = w.units[0].position;
        for _ in 0..60 {
            w.tick();
            let u = &w.units[0];
            assert!(w.regiments[0].engaged.is_empty());
            assert!(near(u.facing, [0.0, 1.0]) && u.position[1] > before[1]);
            before = u.position;
        }
        assert!(!w.units[1].alive() && w.units[0].health < 100.0);
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
        let mut pillbox = building(20, 2, [21.0, 24.0]);
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
        // Each in the other's range, edge to edge, from y 12.1, after 1s.
        run(&mut w, 2.0);
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
        // In the tiles its control range of 18 takes in from 14, after 2s;
        // its rifle reaches from 18.
        run(&mut w, 1.5);
        assert!(w.regiments[0].engaged.is_empty());
        run(&mut w, 1.0);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(w.regiments[0].position[1] < 16.0);
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
    fn guards_rest_at_their_post_ready_to_fight_and_come_back_after() {
        let mut w = skirmish();
        // Unarmed, so it never fires on them to draw them in.
        w.units[1].fighter = Some(soldier(Vec::new()));
        let guard = Command::Guard {
            regiment: 0,
            to: [10.0, 12.0],
        };
        w.apply(1, guard.clone()).unwrap();
        // On the way it's as on a move.
        assert_eq!(w.regiments[0].current_formation(), 0);
        assert!(!w.regiments[0].guarding());
        run(&mut w, 0.5);
        assert!(w.regiments[0].engaged.is_empty());
        // Once there it's in its combat formation, and takes on the enemy
        // 18 off, further than its control range of 18 takes in.
        run(&mut w, 1.0);
        assert_eq!(w.regiments[0].current_formation(), 1);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        run(&mut w, 10.0);
        assert!(!w.units[1].alive());
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [10.0, 12.0]));
        assert!(w.regiments[0].guarding());
        assert_eq!(w.regiments[0].control(&w.guard_rules), 18.0 * 1.25);
        // The same order again changes nothing; another ends it.
        w.apply(1, guard).unwrap();
        assert!(w.regiments[0].guarding());
        w.apply(1, Command::Stop { regiment: 0 }).unwrap();
        assert_eq!(w.regiments[0].guard, None);
        assert_eq!(w.regiments[0].current_formation(), 0);
    }

    /// A guard of player 1's, an ally's regiment for it to guard and an
    /// enemy out of range, 10 apart in a row, the guard noticing nothing
    /// for itself.
    fn escort() -> World {
        let mut w = battle(
            &[1, 3, 2],
            vec![
                soldier(vec![rifle(6.0, 50.0)]),
                soldier(Vec::new()),
                soldier(vec![rifle(6.0, 1.0)]),
            ],
            at_war(),
        );
        w.regiments[0].control_range = 4.0;
        let guard = Command::GuardThing {
            regiment: 0,
            thing: Enemy::Regiment(1),
        };
        w.apply(1, guard).unwrap();
        w
    }

    #[test]
    fn guards_stay_beside_what_they_guard_on_the_side_they_came_from() {
        let mut w = escort();
        run(&mut w, 5.0);
        // Half its control range off, facing away.
        assert!(within(w.units[0].position, [10.0, 18.0], ARRIVAL_REACH));
        assert_eq!(w.units[0].facing, [0.0, -1.0]);
        assert!(w.regiments[0].guarding());
        // An enemy that's fighting nobody is left alone.
        assert_eq!(w.regiments[0].order, Order::Move);
        w.apply(
            3,
            Command::Move {
                regiment: 1,
                to: [30.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        assert!(!w.regiments[0].guarding());
        run(&mut w, 14.0);
        assert!(within(w.units[0].position, [30.0, 18.0], ARRIVAL_REACH));
        assert!(w.regiments[0].guarding());
    }

    #[test]
    fn guards_go_after_whoever_fights_what_they_guard_until_its_gone() {
        let mut w = escort();
        run(&mut w, 5.0);
        w.apply(
            2,
            Command::Attack {
                regiment: 2,
                to: [10.0, 20.0],
            },
        )
        .unwrap();
        w.tick();
        assert!(w.regiments[2].engaged.contains(&Enemy::Regiment(1)));
        assert_eq!(w.regiments[0].order, Order::Pursue(2));
        assert_eq!(w.regiments[0].guard.unwrap().of, Some(Enemy::Regiment(1)));
        run(&mut w, 10.0);
        assert!(w.destroyed(2));
        // Back beside it, on the side the fight left it.
        run(&mut w, 10.0);
        assert!(within(w.units[0].position, [10.0, 22.0], ARRIVAL_REACH));
        assert!(w.regiments[0].guarding());
        w.destroy(1);
        w.tick();
        assert_eq!(w.regiments[0].guard, None);
    }

    #[test]
    fn guards_leave_intruders_they_cant_reach_and_keep_their_post() {
        let mut w = escort();
        run(&mut w, 5.0);
        for x in 0..40 {
            w.pathing.set_blocked(x, 12);
        }
        w.regiments[2].engaged.push(Enemy::Regiment(1));
        w.tick();
        assert_eq!(w.regiments[0].order, Order::Pursue(2));
        // As far as it can get by 3s, then back.
        run(&mut w, 5.0);
        assert_eq!(w.regiments[0].order, Order::Move);
        assert!(w.regiments[2].engaged.contains(&Enemy::Regiment(1)));
        // Back beside it, on the side that left it.
        run(&mut w, 5.0);
        assert!(w.regiments[2].engaged.contains(&Enemy::Regiment(1)));
        assert_eq!(w.regiments[0].order, Order::Move);
        assert!(distance(w.units[0].position, w.centre(1)) < 2.0 + ARRIVAL_REACH);
        assert!(w.regiments[0].guarding());
    }

    #[test]
    fn guards_keep_to_disabled_buildings_and_posts_on_the_map() {
        // 3 from the map's edge, the guard beyond it.
        let mut w = siege(&[building(20, 1, [10.0, 3.0])]);
        w.units[0].position = [10.0, 0.5];
        let guard = Command::GuardThing {
            regiment: 0,
            thing: Enemy::Building(0),
        };
        w.apply(1, guard.clone()).unwrap();
        assert_eq!(w.regiments[0].guard.unwrap().post[1], 0.0);
        w.buildings[0].state = State::Disabled;
        w.tick();
        assert!(w.regiments[0].guard.is_some());
        assert!(w.check(1, &guard).is_ok());
    }

    #[test]
    fn regiments_guard_anything_still_there_but_themselves() {
        let mut w = escort();
        let guard = |regiment, thing| Command::GuardThing { regiment, thing };
        assert!(w.check(1, &guard(0, Enemy::Regiment(2))).is_ok());
        let unable = Err(Refused::Unable);
        assert_eq!(w.check(1, &guard(0, Enemy::Regiment(0))), unable);
        assert_eq!(w.check(1, &guard(0, Enemy::Regiment(9))), unable);
        assert_eq!(w.check(1, &guard(0, Enemy::Building(0))), unable);
        w.destroy(2);
        assert_eq!(w.check(1, &guard(0, Enemy::Regiment(2))), unable);
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
        // Away to 38 by 4s; in the tiles its control range of 18 takes in
        // after 6s.
        w.apply(
            2,
            Command::Move {
                regiment: 1,
                to: [10.0, 38.0],
            },
        )
        .unwrap();
        run(&mut w, 5.5);
        assert!(w.regiments[0].engaged.is_empty());
        assert!(w.units[0].moving());
        run(&mut w, 1.0);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(w.regiments[0].position[1] < 24.0);
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
                soft: Shape::Rect {
                    width: 7.0,
                    height: 7.0,
                },
                target: Shape::Rect {
                    width: 6.0,
                    height: 6.0,
                },
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
                soft: Shape::Rect {
                    width: 7.0,
                    height: 7.0,
                },
            }),
            deploy: None,
            organization: None,
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
        assert_eq!(w.buildings[0].noticed, [Enemy::Regiment(0)]);
        assert_eq!(w.units[0].health, 90.0);
        // Spotting and firing on an enemy isn't being under siege; being
        // hit by one is.
        assert!(w.buildings[0].besieged.is_none());
        assert!(w.units[0].besieged.is_some());
        w.units[0].position = [10.0, 10.0];
        run(&mut w, 2.0);
        assert_eq!(w.units[0].health, 80.0);
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
        w
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
        assert_eq!(w.buildings[0].noticed, [Enemy::Regiment(0)]);
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
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
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
        assert_eq!(captures(&w), [lost(0, 2, 1), captured(0, 1)]);
        let destroyed = Notice::Destroyed {
            owner: 2,
            what: Fallen::Building(1),
        };
        assert_eq!(w.notices.iter().filter(|&n| *n == destroyed).count(), 1);
    }

    /// What `w`'s sides have been told of buildings changing hands.
    fn captures(w: &World) -> Vec<Notice> {
        let notices = w.notices.iter().cloned();
        notices
            .filter(|n| matches!(n, Notice::Lost { .. } | Notice::Captured { .. }))
            .collect()
    }

    fn lost(building: usize, player: u32, by: u32) -> Notice {
        Notice::Lost {
            player,
            building,
            by,
        }
    }

    fn captured(building: usize, player: u32) -> Notice {
        Notice::Captured { player, building }
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
        // Told once, of the centre, not of the house that went with it.
        assert_eq!(captures(&w), [lost(0, 2, 1), captured(0, 1)]);
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
    fn cities_take_members_by_id_when_they_change_hands() {
        let mut w = siege(&city()[..1]);
        // Its member 31 comes after it, and nothing has the id 99.
        w.buildings[0].members.push(99);
        w.add_buildings(&city()[1..]);
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

    #[test]
    fn independent_sides_dont_take_cities() {
        let mut sides = at_war();
        sides.players[0].independent = true;
        let mut w = battle(&[1], vec![soldier(vec![rifle(1.0, 30.0)])], sides);
        w.add_buildings(&city());
        w.units[0].position = [40.0, 15.0];
        run(&mut w, 1.0);
        assert_eq!(w.buildings[0].owner, 2);
        // The same soldier takes it for a side that isn't.
        w.sides.players[0].independent = false;
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
            general: None,
            relations: stored(relations),
            team_relations: stored(team_relations),
            stockpile: 0.0,
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
            soft: Shape::Circle { radius: 2.5 },
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
            0x79087b9feb5389f6,
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

    /// The way buildings face unless told otherwise.
    const NORTH: [f32; 2] = aaa_map::UNPACKED_FACING;

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
            hq: None,
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
                room: Shape::Rect {
                    width: 12.0,
                    height: 12.0,
                },
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
        let put_out = Notice::PutOut {
            player: 1,
            building: 0,
            regiment: 0,
        };
        assert_eq!(w.notices.iter().filter(|&n| *n == put_out).count(), 1);
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
                ..Requires::default()
            },
        );
        let commission = || Command::Commission {
            building: 0,
            commission: Box::new(Commission::new(truck(), 1.0)),
        };
        let lacks = |w: &World| -> Vec<String> {
            w.lacks(1, 0, &truck())
                .structures
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
    fn technologies_grey_or_hide_what_needs_them() {
        let mut w = headquarters();
        w.buildings[0].template = "hq".into();
        let needs = |properties: &[&str], hide: bool| Requires {
            properties: properties.iter().map(|&p| Id::from(p)).collect(),
            hide,
            ..Requires::default()
        };
        // A second member, which decides where the two differ.
        let mut regiment = truck();
        let mut rifleman = regiment.members[0].clone();
        rifleman.thing.template = "rifleman".into();
        regiment.members.push(rifleman);
        let commission = |regiment: &aaa_map::Regiment| Command::Commission {
            building: 0,
            commission: Box::new(Commission::new(regiment.clone(), 1.0)),
        };
        w.requires
            .insert(Id::from("truck"), needs(&["tech_heavy"], false));
        w.requires
            .insert(Id::from("rifleman"), needs(&["boat"], true));
        let lacking = w.lacks(1, 0, &regiment);
        assert_eq!(
            lacking.properties,
            [Id::from("tech_heavy"), Id::from("boat")]
        );
        assert!(lacking.hidden);
        assert_eq!(w.apply(1, commission(&regiment)), Err(Refused::Lacking));
        // What the recruiter carries itself counts only for it.
        w.kinds.insert(
            Id::from("hq"),
            aaa_map::Kind {
                properties: vec![Id::from("boat")],
                ..aaa_map::Kind::default()
            },
        );
        let lacking = w.lacks(1, 0, &regiment);
        assert_eq!(lacking.properties, [Id::from("tech_heavy")]);
        assert!(!lacking.hidden);
        // A technology counts once the side holds it, and only for that side.
        w.grant(2, Id::from("tech_heavy"));
        assert!(!w.lacks(1, 0, &regiment).is_empty());
        w.grant(1, Id::from("tech_heavy"));
        assert!(w.lacks(1, 0, &regiment).is_empty());
        w.apply(1, commission(&regiment)).unwrap();
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
        assert_eq!(
            w.order_unpack(1, r, [22.0, 22.0], NORTH),
            Err(Refused::NoRoom)
        );
        assert_eq!(
            w.order_unpack(1, r, [28.0, 20.0], NORTH),
            Err(Refused::NoRoom)
        );
        assert_eq!(
            w.order_unpack(1, r, [78.0, 30.0], NORTH),
            Err(Refused::NoRoom)
        );
        assert_eq!(
            w.order_unpack(2, r, [30.0, 40.0], NORTH),
            Err(Refused::NotYours)
        );
        assert_eq!(w.order_unpack(1, r, [30.2, 39.6], NORTH), Ok([30.0, 40.0]));
        run(&mut w, 3.0);
        assert!(w.units[0].alive());
        assert_eq!(w.buildings.len(), 1);
        run(&mut w, 1.0);
        assert!(!w.units[0].alive());
        assert!(w.units[0].died.is_none());
        assert!(w.destroyed(r));
        let depot = &w.buildings[1];
        assert_eq!((depot.template.as_str(), depot.owner), ("depot", 1));
        // It takes over the truck's id.
        assert_eq!(depot.id, 9);
        assert_eq!(depot.position, [30.0, 40.0]);
        let (from, to) = depot.unpacking.unwrap();
        assert_eq!(to - from, 70);
        assert!(!w.pathing.passable(15, 20, Domain::Land));
        run(&mut w, 7.0);
        assert!(w.buildings[1].unpacking.is_none());
    }

    #[test]
    fn unpacking_clears_the_scenery_under_the_building() {
        let mut w = headquarters();
        let tree = |at: [f32; 2]| Obstacle {
            position: at,
            facing: NORTH,
            shape: Shape::Circle { radius: 0.2 },
            soft: Shape::Circle { radius: 0.5 },
        };
        // The depot will cover 26 to 34 across: one tree stands within it,
        // one reaches into its edge, one is just clear of it though within
        // the room it needs, and one is further off.
        w.clearable = vec![
            (50, tree([31.0, 41.0])),
            (51, tree([34.1, 40.0])),
            (52, tree([34.3, 40.0])),
            (53, tree([40.0, 40.0])),
        ];
        let mut t = truck();
        t.owner = 1;
        t.renumber(9);
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        // The trees don't stand in its way.
        w.order_unpack(1, r, [30.0, 40.0], NORTH).unwrap();
        run(&mut w, 2.0);
        assert!(w.cleared.is_empty());
        run(&mut w, 2.0);
        assert_eq!(w.buildings.len(), 2);
        assert_eq!(w.cleared, [50, 51]);
        let ids: Vec<u32> = w.clearable.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [52, 53]);
    }

    #[test]
    fn trucks_that_turn_over_time_unpack_behind_and_beside_themselves() {
        for at in [[30.0, 24.0], [34.0, 30.0], [27.0, 31.0]] {
            let mut w = World::from_regiments(at_war(), Pathing::uniform(40, Domain::Land), &[]);
            let mut t = truck();
            t.owner = 1;
            // Round a radius of 3, as the game's trucks turn.
            t.members[0].turn_rate = 1.0;
            t.renumber(9);
            t.place([30.0, 30.0], [0.0, 1.0]);
            let r = w.add_regiment(&t);
            w.order_unpack(1, r, at, NORTH).unwrap();
            run(&mut w, 20.0);
            assert_eq!(w.buildings.len(), 1, "{at:?}: {:?}", w.units[0].position);
            assert_eq!(w.buildings[0].position, at);
        }
    }

    #[test]
    fn trucks_left_no_room_ask_again_then_say_they_cant_unpack() {
        let mut w = headquarters();
        let mut t = truck();
        t.owner = 1;
        t.renumber(9);
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        w.order_unpack(1, r, [30.0, 40.0], NORTH).unwrap();
        // Something goes up there before it arrives.
        w.add_buildings(&[building(40, 1, [30.0, 40.0])]);
        run(&mut w, 4.0);
        assert!(w.regiments[r].unpack_at.is_some() && w.notices.is_empty());
        run(&mut w, 1.5);
        assert!(w.regiments[r].unpack_at.is_none() && w.units[0].alive());
        let told = Notice::CannotUnpack {
            player: 1,
            regiment: r,
        };
        assert_eq!(w.notices, [told]);
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
                facing: NORTH,
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        w.apply(1, Command::Stop { regiment: r }).unwrap();
        run(&mut w, 5.0);
        assert!(w.units[0].alive());
        assert_eq!(w.buildings.len(), 1);
    }

    /// A regiment of one truck.
    fn convoy() -> aaa_map::Regiment {
        aaa_map::Regiment {
            hq: None,
            template: "convoy".into(),
            ..truck()
        }
    }

    #[test]
    fn trucks_unpack_facing_the_way_theyre_told() {
        let mut w = headquarters();
        let mut t = truck();
        t.owner = 1;
        let unpacks = t.unpacks.as_mut().unwrap();
        let long = Shape::Rect {
            width: 4.0,
            height: 16.0,
        };
        let obstacle = unpacks.building.obstacle.as_mut().unwrap();
        (obstacle.shape, obstacle.soft) = (long, long);
        unpacks.room = Shape::Rect {
            width: 4.0,
            height: 20.0,
        };
        t.place([30.0, 60.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        // Long side north to south it would reach the map's edge; east to
        // west it fits.
        assert_eq!(
            w.order_unpack(1, r, [30.0, 70.0], NORTH),
            Err(Refused::NoRoom)
        );
        assert_eq!(
            w.order_unpack(1, r, [30.0, 70.0], [2.0, 0.0]),
            Ok([30.0, 70.0])
        );
        run(&mut w, 5.0);
        let depot = &w.buildings[1];
        assert_eq!(depot.facing, [1.0, 0.0]);
        assert_eq!(depot.obstacle.as_ref().unwrap().facing, [1.0, 0.0]);
        assert!(!w.pathing.passable(12, 35, Domain::Land));
        assert!(w.pathing.passable(15, 32, Domain::Land));
    }

    #[test]
    fn buildings_pack_back_into_their_place_in_an_organization() {
        let mut w = headquarters();
        w.packing.insert("depot".into(), packs());
        let mut c = convoy();
        c.owner = 1;
        c.renumber(30);
        c.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&c);
        let u = w.regiments[r].units[0];
        w.apply(
            1,
            Command::Unpack {
                regiment: r,
                at: [30.0, 40.0],
                facing: NORTH,
            },
        )
        .unwrap();
        run(&mut w, 11.0);
        let depot = w.regiments[r].unpacked_into.unwrap();
        assert_eq!(w.buildings[depot].id, 31);
        w.buildings[depot].health *= 0.8;
        w.apply(1, Command::Pack { building: depot }).unwrap();
        run(&mut w, 7.1);
        let State::Packed(_, packed) = w.buildings[depot].state else {
            panic!("not packed: {:?}", w.buildings[depot].state);
        };
        // The same regiment, with the same truck, back where the depot stood.
        assert_eq!(packed, r);
        assert_eq!((w.regiments.len(), w.units.len()), (1, 1));
        let regiment = &w.regiments[r];
        assert_eq!((regiment.id, regiment.template.as_str()), (30, "convoy"));
        assert_eq!(regiment.unpacked_into, None);
        assert!(!w.destroyed(r));
        assert_eq!(w.units[u].id, 31);
        assert!(near(w.units[u].position, [30.0, 40.0]));
        assert!(near(w.units[u].facing, [1.0, 0.0]));
        assert!((w.units[u].health - 80.0).abs() < 1e-3);
        // And it can go and unpack again.
        w.apply(
            1,
            Command::Unpack {
                regiment: r,
                at: [30.0, 60.0],
                facing: NORTH,
            },
        )
        .unwrap();
        run(&mut w, 15.0);
        assert_eq!(w.regiments[r].unpacked_into, Some(2));
    }

    /// The depot packing into [`truck`], facing east.
    fn packs() -> Packs {
        Packs {
            truck: truck(),
            offset: [0.0, 0.0],
            facing: [1.0, 0.0],
            seconds: 7.0,
            health_min: 0.5,
        }
    }

    #[test]
    fn captured_buildings_pack_into_trucks_for_their_new_owner() {
        let mut w = headquarters();
        w.packing.insert("depot".into(), packs());
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        w.apply(
            1,
            Command::Unpack {
                regiment: r,
                at: [30.0, 40.0],
                facing: NORTH,
            },
        )
        .unwrap();
        run(&mut w, 11.0);
        let depot = w.regiments[r].unpacked_into.unwrap();
        w.change_hands(depot, 2);
        w.apply(2, Command::Pack { building: depot }).unwrap();
        run(&mut w, 7.1);
        assert!(matches!(w.buildings[depot].state, State::Packed(_, p) if p == r));
        assert_eq!(w.regiments[r].owner, 2);
    }

    #[test]
    fn numbered_regiments_count_per_side_and_template_never_reusing_one() {
        let mut w = headquarters();
        let mut t = truck();
        let mut add = |w: &mut World, owner, template: &str| {
            (t.owner, t.template) = (owner, template.into());
            let r = w.add_regiment(&t);
            w.regiments[r].number
        };
        // Those already here are numbered once the templates are known.
        add(&mut w, 1, "truck");
        add(&mut w, 1, "truck");
        assert_eq!(w.regiments[1].number, None);
        w.set_numbered(HashSet::from([Id::from("truck"), Id::from("tank")]));
        assert_eq!(
            (w.regiments[0].number, w.regiments[1].number),
            (Some(1), Some(2))
        );
        assert_eq!(add(&mut w, 2, "truck"), Some(1));
        assert_eq!(add(&mut w, 1, "tank"), Some(1));
        assert_eq!(add(&mut w, 1, "jeep"), None);
        // Nobody's aren't numbered.
        assert_eq!(add(&mut w, 0, "truck"), None);
        w.destroy(1);
        assert_eq!(add(&mut w, 1, "truck"), Some(3));
    }

    #[test]
    fn buildings_a_map_has_pack_into_their_place_there() {
        let mut w = headquarters();
        w.packing.insert("depot".into(), packs());
        let depot = |id, x| {
            let mut depot = truck().unpacks.unwrap().building;
            (depot.id, depot.owner, depot.position) = (id, 1, [x, 40.0]);
            depot.obstacle.as_mut().unwrap().position = depot.position;
            depot
        };
        // One standing in an organization, with another member beside it,
        // and one on its own.
        let mut within = depot(41, 30.0);
        within.organization = Some(Box::new(aaa_map::Regiment {
            hq: None,
            id: 40,
            template: "hq_unpacked".into(),
            members: Vec::new(),
            ..convoy()
        }));
        w.set_numbered(HashSet::from([Id::from("hq_unpacked")]));
        w.add_buildings(&[within, depot(50, 60.0)]);
        assert_eq!(
            (w.buildings[1].number, w.buildings[2].number),
            (Some(1), None)
        );
        for b in [1, 2] {
            w.apply(1, Command::Pack { building: b }).unwrap();
        }
        run(&mut w, 7.1);
        let packed = |b: usize| match w.buildings[b].state {
            State::Packed(_, r) => &w.regiments[r],
            ref state => panic!("not packed: {state:?}"),
        };
        // The truck takes the building's id and place in the organization,
        // which keeps its own.
        let r = packed(1);
        assert_eq!((r.id, r.template.as_str(), r.owner), (40, "hq_unpacked", 1));
        // It keeps the building's number rather than taking the next.
        assert_eq!(r.number, Some(1));
        assert_eq!(w.units[r.units[0]].id, 41);
        assert!(near(w.units[r.units[0]].position, [30.0, 40.0]));
        assert!(r.unpacks.is_some());
        // Alone it packs into a lone truck under its id.
        let r = packed(2);
        assert_eq!((r.id, r.template.as_str()), (50, "truck"));
        assert_eq!(w.units[r.units[0]].id, 50);
        assert_eq!(w.regiments.len(), 2);
    }

    #[test]
    fn buildings_pack_back_into_their_trucks() {
        let mut w = headquarters();
        w.packing.insert("depot".into(), packs());
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        w.set_numbered(HashSet::from([Id::from("truck")]));
        w.add_regiment(&t);
        let r = w.add_regiment(&t);
        assert_eq!(w.regiments[r].number, Some(2));
        w.apply(
            1,
            Command::Unpack {
                regiment: r,
                at: [30.0, 40.0],
                facing: NORTH,
            },
        )
        .unwrap();
        run(&mut w, 4.0);
        let depot = w.regiments[r].unpacked_into.unwrap();
        // The building is the truck to the player, number and all.
        assert_eq!(w.buildings[depot].number, Some(2));
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
        // A lone truck's building stands in no organization, so packs into
        // a lone truck: the one it unpacked from, under its old id.
        assert_eq!(packed, r);
        assert_eq!(w.regiments.len(), 2);
        assert_eq!(w.regiments[packed].template, "truck");
        assert_eq!(w.regiments[packed].number, Some(2));
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
                facing: NORTH,
            },
        )
        .unwrap();
    }

    fn amounts(of: &[(&str, f32)]) -> aaa_map::Amounts {
        let mut a = aaa_map::Amounts::default();
        for &(kind, amount) in of {
            a.set(kind, amount);
        }
        a
    }

    /// [`headquarters`] earning and keeping up like a Corps HQ, its trucks
    /// like a Division HQ's and the depot they unpack into earning twice
    /// theirs, with the game's limits and shortfall prices, and player 1
    /// holding 500.
    fn economy() -> World {
        let mut w = headquarters();
        let economy = |cost, production, upkeep| aaa_map::Economy {
            cost: amounts(cost),
            production: amounts(production),
            upkeep: amounts(upkeep),
            ..Default::default()
        };
        w.economies.insert(
            Id::from("house"),
            economy(
                &[],
                &[("money", 25.0), ("ammo", 20.0), ("oil", 10.0)],
                &[("building_limit_consumed", 1.0)],
            ),
        );
        w.economies.insert(
            Id::from("truck"),
            economy(
                &[("money", 250.0)],
                &[
                    ("money", 12.0),
                    ("ammo", 5.0),
                    ("oil", 2.0),
                    ("unit_limit_provided", 6.0),
                ],
                &[("building_limit_consumed", 1.0)],
            ),
        );
        w.economies.insert(
            Id::from("depot"),
            economy(
                &[],
                &[
                    ("money", 25.0),
                    ("ammo", 10.0),
                    ("oil", 5.0),
                    ("unit_limit_provided", 6.0),
                ],
                &[("building_limit_consumed", 1.0)],
            ),
        );
        w.economy = aaa_map::EconomyRules {
            purchase_cost: amounts(&[("ammo", 2.0), ("oil", 3.0)]),
            limits: vec![
                ("unit_limit_consumed", "unit_limit_provided"),
                ("building_limit_consumed", "building_limit_provided"),
            ],
            limited_max: 20.0,
            stockpile_max: 99999.0,
            ..Default::default()
        };
        w.set_stockpile(1, 500.0);
        w
    }

    /// A regiment of three soldiers of `owner`'s, which with each soldier
    /// keeps up ammo.
    fn company(w: &mut World, owner: u32) -> usize {
        let regiment = raised_company(w, owner);
        w.add_regiment(&regiment)
    }

    /// A [`company`] yet to be put on the map, taking a place under the
    /// regiment limit.
    fn raised_company(w: &mut World, owner: u32) -> aaa_map::Regiment {
        let members = (0..3).map(|i| member(20 + i, [5.0 + i as f32, 5.0], 1.0));
        let mut regiment = truck();
        regiment.template = "company".into();
        regiment.owner = owner;
        regiment.members = members.collect();
        regiment.unpacks = None;
        regiment.primary = None;
        regiment.formations[0].offsets = vec![[0.0, 0.0]; 3];
        let economy = |upkeep| aaa_map::Economy {
            upkeep: amounts(upkeep),
            ..Default::default()
        };
        w.economies.insert(
            Id::from("company"),
            economy(&[("ammo", 10.0), ("unit_limit_consumed", 1.0)]),
        );
        w.economies
            .insert(Id::from("soldier"), economy(&[("ammo", 2.0)]));
        regiment
    }

    #[test]
    fn sides_earn_what_they_own_produces_less_its_upkeep() {
        let mut w = economy();
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        w.add_regiment(&t);
        let r = company(&mut w, 1);
        company(&mut w, 2);
        let balance = w.balance(1);
        assert_eq!(balance.net.get("money"), 37.0);
        // The company keeps up its own 10 and its three soldiers' 2 each.
        assert_eq!(balance.net.get("ammo"), 25.0 - 16.0);
        assert_eq!(balance.net.get("oil"), 12.0);
        assert_eq!(
            balance.limits,
            [
                Limit {
                    consumed: "unit_limit_consumed",
                    used: 1.0,
                    provided: 6.0
                },
                Limit {
                    consumed: "building_limit_consumed",
                    used: 2.0,
                    provided: 0.0
                },
            ]
        );
        // A lost member's place costs as before, until the regiment's gone.
        w.units[w.regiments[r].units[0]].die(0);
        assert_eq!(w.balance(1).net.get("ammo"), 9.0);
        w.destroy(r);
        assert_eq!(w.balance(1).net.get("ammo"), 25.0);
        // A disabled building earns nothing but is still kept up.
        w.buildings[0].state = State::Disabled;
        let ledger = w.ledger(1);
        assert_eq!(ledger.production.get("money"), 12.0);
        assert_eq!(ledger.upkeep.get("building_limit_consumed"), 2.0);
    }

    #[test]
    fn shortfalls_are_bought_with_money() {
        let mut w = economy();
        for _ in 0..4 {
            company(&mut w, 1);
        }
        // 64 ammo kept up against 20 made, so 44 bought at 2 each.
        let balance = w.balance(1);
        assert_eq!(balance.net.get("ammo"), -44.0);
        assert_eq!(balance.net.get("money"), 25.0 - 88.0);
        assert_eq!((balance.income, balance.costs), (25.0, 88.0));
        // Limits provided are capped.
        w.economies
            .get_mut(&Id::from("house"))
            .unwrap()
            .production
            .set("unit_limit_provided", 30.0);
        assert_eq!(w.balance(1).limits[0].provided, 20.0);
    }

    #[test]
    fn stockpiles_fill_with_what_sides_net_and_tell_them_when_short() {
        let mut w = economy();
        run(&mut w, 60.0);
        assert!((w.stockpile(1) - 525.0).abs() < 0.01);
        w.set_stockpile(1, 1.0);
        for _ in 0..4 {
            company(&mut w, 1);
        }
        // Netting -63 a minute, 1 lasts under a second.
        run(&mut w, 1.0);
        assert_eq!(w.stockpile(1), 0.0);
        let percent = (88.0 - 25.0) / 88.0 * 100.0;
        assert_eq!(w.notices, [Notice::Short { player: 1, percent }]);
        // Told again only once it moves on by more than 5 points.
        let earn = |w: &mut World, money| {
            let house = w.economies.get_mut(&Id::from("house")).unwrap();
            house.production.set("money", money);
            run(w, 1.0);
        };
        earn(&mut w, 27.0);
        assert_eq!(w.notices.len(), 1);
        earn(&mut w, 15.0);
        assert_eq!(w.notices.len(), 2);
        w.set_stockpile(1, 99999.0);
        run(&mut w, 1.0);
        assert_eq!(w.notices[2], Notice::Repaired { player: 1 });
        // Never more than the most a stockpile holds.
        w.set_stockpile(1, 200000.0);
        assert_eq!(w.stockpile(1), 99999.0);
    }

    #[test]
    fn commissions_are_paid_for_up_front_and_refunded_if_called_off() {
        let mut w = economy();
        let order = |w: &mut World| {
            w.apply(
                1,
                Command::Commission {
                    building: 0,
                    commission: Box::new(Commission::new(truck(), 10.0)),
                },
            )
        };
        order(&mut w).unwrap();
        order(&mut w).unwrap();
        assert_eq!(w.stockpile(1), 0.0);
        assert_eq!(order(&mut w), Err(Refused::Unaffordable));
        let second = w.buildings[0].queue[1].id;
        w.apply(
            1,
            Command::CancelCommission {
                building: 0,
                commission: second,
            },
        )
        .unwrap();
        assert_eq!(w.stockpile(1), 250.0);
    }

    #[test]
    fn previews_show_the_economy_after_an_order() {
        let mut w = economy();
        let (stockpile, balance) = w.preview(1, Change::Commission(&truck()));
        assert_eq!(stockpile, 250.0);
        assert_eq!(balance.net.get("money"), 37.0);
        assert_eq!(balance.limits[0].provided, 6.0);
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        // Unpacking swaps the truck's production for the depot's.
        let (stockpile, balance) = w.preview(1, Change::Unpack(r));
        assert_eq!(stockpile, 500.0);
        assert_eq!(balance.net.get("money"), 50.0);
        assert_eq!(balance.limits[1].used, 2.0);
    }

    #[test]
    fn start_locations_give_their_side_room_to_build() {
        let mut w = economy();
        w.economies.insert(
            Id::from("start_loc"),
            aaa_map::Economy {
                production: amounts(&[("building_limit_provided", 20.0)]),
                ..Default::default()
            },
        );
        w.holdings.push((1, "start_loc".into()));
        let building = w.balance(1).limits[1];
        assert_eq!((building.used, building.provided), (1.0, 20.0));
    }

    #[test]
    fn commissions_wait_at_the_front_of_the_queue_for_room_under_limits() {
        let mut w = economy();
        let company = raised_company(&mut w, 1);
        // An order can't say it has its share of the limits already.
        let mut commission = Commission::new(company, 1.0);
        commission.held = true;
        let order = Command::Commission {
            building: 0,
            commission: Box::new(commission),
        };
        // Accepted and paid for though there's no room.
        w.apply(1, order).unwrap();
        for _ in 0..20 {
            w.tick();
        }
        assert_eq!(w.buildings[0].progress, 0);
        assert_eq!(
            w.buildings[0].paused,
            Some(Pause::Limit("unit_limit_consumed"))
        );
        let paused = |w: &World| {
            w.notices
                .iter()
                .filter(|n| matches!(n, Notice::Paused { player: 1, .. }))
                .count()
        };
        assert_eq!(paused(&w), 1, "told once");
        // Room once a truck provides it, taken while it's raised.
        let mut t = truck();
        t.owner = 1;
        t.place([30.0, 30.0], [0.0, 1.0]);
        w.add_regiment(&t);
        w.tick();
        assert_eq!(w.buildings[0].paused, None);
        assert_eq!(w.balance(1).limits[0].used, 1.0);
        for _ in 0..10 {
            w.tick();
        }
        assert!(w.buildings[0].queue.is_empty());
        assert_eq!(
            w.balance(1).limits[0].used,
            1.0,
            "the raised company holds it"
        );
    }

    #[test]
    fn disabled_buildings_hold_their_queues() {
        let mut w = economy();
        let order = Command::Commission {
            building: 0,
            commission: Box::new(Commission::new(truck(), 1.0)),
        };
        w.apply(1, order).unwrap();
        w.tick();
        w.buildings[0].state = State::Disabled;
        for _ in 0..20 {
            w.tick();
        }
        assert_eq!(w.buildings[0].queue.len(), 1);
        assert_eq!(w.buildings[0].paused, Some(Pause::Disabled));
        assert!(w.notices.iter().any(|n| matches!(
            n,
            Notice::Paused {
                why: Pause::Disabled,
                ..
            }
        )));
    }

    #[test]
    fn queues_are_refunded_to_their_owner_when_their_building_changes_hands() {
        let mut w = economy();
        let order = Command::Commission {
            building: 0,
            commission: Box::new(Commission::new(truck(), 10.0)),
        };
        w.apply(1, order).unwrap();
        assert_eq!(w.stockpile(1), 250.0);
        w.change_hands(0, 2);
        assert!(w.buildings[0].queue.is_empty());
        assert_eq!(w.stockpile(1), 500.0);
        assert_eq!(w.stockpile(2), 0.0);
    }

    fn supply(range: f32, relay: bool, stockpile_max: f32) -> aaa_map::Supply {
        aaa_map::Supply {
            range,
            relay,
            stockpile_max,
        }
    }

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
        w.supplies
            .insert(Id::from("corps"), supply(20.0, false, 0.0));
        w.supplies
            .insert(Id::from("depot"), supply(20.0, true, 3.0));
        w
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
        w.supplies
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
        w.packing.insert("depot".into(), packs());
        let mut depot = truck().unpacks.unwrap().building;
        (depot.id, depot.owner, depot.position) = (41, 1, [50.0, 50.0]);
        depot.obstacle.as_mut().unwrap().position = depot.position;
        depot.deploy = w.buildings[0].deploy;
        w.add_buildings(&[depot]);
        w.supplies
            .insert(Id::from("depot"), supply(20.0, true, 2.0));
        let room = aaa_map::Economy {
            production: amounts(&[("building_limit_provided", 20.0)]),
            ..Default::default()
        };
        w.economies.insert(Id::from("start_loc"), room);
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
        w.supplies
            .insert(Id::from("house"), supply(60.0, false, 0.0));
        w.buildings[0].supply = Supplying::default();
        run(&mut w, 3.0);
        assert_eq!(w.building_ledger(1).production.get("money"), 25.0);
        assert!(w.buildings[1].queue.is_empty());
        // What can't pack earns in full without supply unless it needs it.
        w.packing.clear();
        w.supplies.remove(&Id::from("house"));
        run(&mut w, 4.0);
        assert!(w.out_of_supply(1));
        assert_eq!(w.building_ledger(1).production.get("money"), 25.0);
        w.economies
            .get_mut(&Id::from("depot"))
            .unwrap()
            .supply_required = true;
        assert_eq!(w.building_ledger(1).production.get("money"), 0.0);
    }

    fn logistics(attachments: Option<u32>) -> aaa_map::Logistics {
        aaa_map::Logistics {
            supply_used: attachments.is_none(),
            detached: false,
            shortfall_damage: 0.01,
            attachments,
        }
    }

    /// [`economy`] with its HQ supplying 30 around it, a Division HQ's
    /// truck of player 1's that takes two companies at 30,30 (regiment 0)
    /// and a company of three at 26,20 (regiment 1), detached.
    fn division() -> World {
        let mut w = economy();
        w.supplies
            .insert(Id::from("house"), supply(30.0, false, 0.0));
        w.logistics.insert(Id::from("truck"), logistics(Some(2)));
        w.logistics.insert(Id::from("company"), logistics(None));
        w.resupply_rates.insert(Id::from("soldier"), 0.5);
        w.recruit_lists
            .insert(Id::from("depot"), vec![Id::from("company")]);
        let mut t = truck();
        (t.id, t.owner) = (50, 1);
        t.members[0].thing.id = 50;
        t.place([30.0, 30.0], [0.0, 1.0]);
        w.add_regiment(&t);
        let mut c = raised_company(&mut w, 1);
        c.renumber(60);
        c.primary = Some(0);
        c.control_range = Some(18.0);
        for m in &mut c.members {
            m.fighter = Some(soldier(Vec::new()));
        }
        c.place([26.0, 20.0], [0.0, 1.0]);
        w.add_regiment(&c);
        for u in w.regiments[1].units.clone() {
            w.units[u].template = "soldier".into();
        }
        w
    }

    #[test]
    fn regiments_attach_to_hqs_that_recruit_their_kind_and_have_room() {
        let mut w = division();
        assert_eq!(w.hqs().len(), 1);
        assert_eq!(w.hqs()[0].id, 50);
        // Packed, it's its regiment, and the company is the one detached.
        assert_eq!((w.packed_hq(0), w.packed_hq(1)), (Some(50), None));
        assert_eq!((w.detached(1), w.detached(2)), (vec![1], vec![]));
        assert!(w.attachable(1) && !w.attachable(2));
        // The HQ itself is in no supply and attaches to nothing.
        assert_eq!(
            w.check(1, &Command::Attach { regiment: 0 }),
            Err(Refused::Unable)
        );
        assert_eq!(
            w.check(1, &Command::Detach { regiment: 1 }),
            Err(Refused::Unable)
        );
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        assert_eq!(w.regiments[1].hq, Some(50));
        assert_eq!(w.attached(50).collect::<Vec<_>>(), [1]);
        // No other HQ to step on to.
        assert_eq!(
            w.check(1, &Command::Attach { regiment: 1 }),
            Err(Refused::Unable)
        );
        // A second one, nearer: a detached regiment takes the nearest, an
        // attached one the next round.
        let mut t = truck();
        (t.id, t.owner) = (70, 1);
        t.members[0].thing.id = 70;
        t.place([26.0, 24.0], [0.0, 1.0]);
        w.add_regiment(&t);
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        assert_eq!(w.regiments[1].hq, Some(70));
        assert!(w.detached(1).is_empty());
        w.notices.clear();
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        assert_eq!(w.regiments[1].hq, Some(50));
        // The player's told of each, a move as leaving then joining.
        let (player, regiment) = (1, 1);
        let left = |hq| Notice::Detached {
            player,
            hq,
            regiment,
        };
        let joined = |hq| Notice::Attached {
            player,
            hq,
            regiment,
        };
        assert_eq!(w.notices, [left(70), joined(50)]);
        w.apply(1, Command::Detach { regiment: 1 }).unwrap();
        assert_eq!(w.notices[2..], [left(50)]);
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        assert_eq!(w.regiments[1].hq, Some(70));
        // Not an enemy's, nor one that doesn't recruit its kind.
        w.regiments[1].hq = None;
        w.regiments[0].owner = 2;
        w.recruit_lists.clear();
        assert_eq!(w.attaches_to(1), None);
    }

    #[test]
    fn regiments_no_hq_recruits_are_detached_too() {
        let mut w = division();
        // Agents, which nothing recruits, and a truck outside any
        // organization.
        w.logistics.insert(Id::from("agents"), logistics(None));
        let mut agents = raised_company(&mut w, 1);
        agents.renumber(80);
        agents.template = "agents".into();
        agents.place([20.0, 20.0], [0.0, 1.0]);
        w.add_regiment(&agents);
        let mut lorry = truck();
        (lorry.id, lorry.owner, lorry.template) = (90, 1, "lorry".into());
        lorry.members[0].thing.id = 90;
        lorry.place([22.0, 30.0], [0.0, 1.0]);
        w.add_regiment(&lorry);
        assert_eq!(w.detached(1), [1, 2]);
        // Only the company is one an HQ could take.
        assert!(w.attachable(1));
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        assert_eq!(w.detached(1), [2]);
        assert!(!w.attachable(1));
    }

    #[test]
    fn regiments_lose_their_hq_when_its_destroyed() {
        let mut w = division();
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        run(&mut w, 1.0);
        assert!(w.regiments[1].in_supply);
        w.destroy(0);
        run(&mut w, 1.0);
        assert_eq!(w.regiments[1].hq, None);
        assert!(!w.regiments[1].in_supply);
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
        w.supplies
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
        w.recruit_lists
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
        w.economies.get_mut(&Id::from("house")).unwrap().upkeep = amounts(&[("money", 500.0)]);
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
            m.thing.template = "soldier".into();
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

    /// [`division`] a couple of seconds on, its company attached and in
    /// supply.
    fn supplied_division() -> World {
        let mut w = division();
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        run(&mut w, 2.0);
        assert!(w.regiments[1].in_supply);
        w
    }

    fn effect(range: f32, resupply: f32, supplied: bool, requires: &[&str]) -> SupplyEffect {
        SupplyEffect {
            range,
            resupply,
            supplied,
            requires: requires.iter().map(|&p| Id::from(p)).collect(),
        }
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
        w.supply_effects
            .insert(Id::from("logistics"), effect(5.0, 0.0, false, &[]));
        w.supply_effects
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
        w.supply_effects.insert(
            Id::from("foraging"),
            effect(0.0, 0.25, false, &["org_unit"]),
        );
        w.supply_effects
            .insert(Id::from("rations"), effect(0.0, 0.25, false, &[]));
        w.supply_effects
            .insert(Id::from("famine"), effect(0.0, -3.0, false, &[]));
        // Only on those with the properties a technology needs.
        w.grant(1, Id::from("foraging"));
        assert_eq!(w.resupply_multiplier(1), 1.0);
        let kind = aaa_map::Kind {
            properties: vec![Id::from("org_unit")],
            ..Default::default()
        };
        w.kinds.insert(Id::from("company"), kind);
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
        w.kinds.insert(Id::from("company"), kind);
        w.supply_effects
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
        w.operations.insert(Id::from("resupply"), operation);
        (w, Id::from("resupply"))
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
        w.economies.get_mut(&Id::from("house")).unwrap().upkeep = amounts(&[("money", 500.0)]);
        w.set_stockpile(1, 0.0);
        run(&mut w, 2.0);
        assert!(w.short(1).is_some() && !w.regiments[1].in_supply);
        // Once it's run out the regiment's as it was.
        w.economies.get_mut(&Id::from("house")).unwrap().upkeep = amounts(&[]);
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
        w.drops.insert(Id::from("provisions"), provisions);
        w.supplies
            .insert(Id::from("provisions"), supply(10.0, false, 0.0));
        w.decays.insert(Id::from("provisions"), 10.0);
        let drop = Id::from("drop");
        let dropped = aaa_map::Operation::Drop(Id::from("provisions"));
        w.operations.insert(drop.clone(), dropped);
        w.operations.insert(
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
        assert_eq!(w.morale_bonus(1), w.supply_rules.morale_bonus);
        assert!(w.morale_bonus(1) > 0.0);
    }

    #[test]
    fn regiments_that_change_side_keep_their_place_at_their_hq() {
        let mut w = supplied_division();
        let hq = w.hqs().remove(0);
        // With an ally, its old side's zones are still its own.
        w.change_side(1, 3);
        run(&mut w, 1.0);
        assert_eq!(w.regiments[1].hq, Some(50));
        assert!(w.regiments[1].in_supply);
        assert_eq!(w.free_places(&hq), 1);
        // With an enemy it's still attached, but needs that side's zones.
        w.change_side(1, 2);
        run(&mut w, 1.0);
        assert_eq!(w.regiments[1].hq, Some(50));
        assert!(!w.regiments[1].in_supply);
        assert_eq!(w.free_places(&hq), 1);
        // An HQ that changes side loses its regiments at once.
        w.change_side(0, 3);
        assert_eq!(w.regiments[1].hq, None);
    }

    #[test]
    fn captured_hqs_lose_their_regiments_at_once() {
        let mut w = division();
        let hq = Command::Unpack {
            regiment: 0,
            at: [30.0, 40.0],
            facing: NORTH,
        };
        w.apply(1, hq).unwrap();
        run(&mut w, 30.0);
        let b = w.regiments[0].unpacked_into.expect("unpacked");
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        w.change_hands(b, 2);
        assert_eq!(w.regiments[1].hq, None);
    }

    /// Players 1 and 2 at war, as `battle` has them, seeing `range` round
    /// themselves and showing those fighting them `remote` round them.
    fn sighted(fighters: Vec<Fighter>, range: f32, remote: f32, fog: aaa_map::FogSetting) -> World {
        let mut w = battle(&[1, 2], fighters, at_war());
        w.sights
            .insert(Id::from("squad"), aaa_map::Sight { range, remote });
        w.set_fog(fog);
        w
    }

    fn put(w: &mut World, u: usize, at: [f32; 2]) {
        w.units[u].position = at;
        w.units[u].previous = at;
    }

    #[test]
    fn sides_and_their_allies_see_round_their_things_and_remember_it() {
        let mut w = sighted(
            vec![soldier(vec![]), soldier(vec![])],
            6.0,
            3.0,
            aaa_map::FogSetting::Full,
        );
        put(&mut w, 1, [70.0, 70.0]);
        w.tick();
        // Tile centres within 6.5 of the centre of the tile under it.
        assert_eq!(w.seen(1, [10.0, 10.0]), Seen::Visible);
        assert_eq!(w.seen(1, [10.0, 17.0]), Seen::Visible);
        assert_eq!(w.seen(1, [10.0, 19.0]), Seen::Unexplored);
        assert_eq!(w.seen(3, [10.0, 10.0]), Seen::Visible);
        assert_eq!(w.seen(2, [10.0, 10.0]), Seen::Unexplored);
        assert!(w.sees_unit(1, 0) && w.sees_unit(3, 0) && !w.sees_unit(2, 0));
        assert!(!w.sees_unit(1, 1));

        put(&mut w, 0, [40.0, 10.0]);
        w.tick();
        assert_eq!(w.seen(1, [10.0, 10.0]), Seen::Explored);
        assert_eq!(w.seen(1, [40.0, 10.0]), Seen::Visible);
        // Until the second comes near enough to be seen.
        put(&mut w, 1, [44.0, 10.0]);
        w.tick();
        assert!(w.sees_unit(1, 1) && w.sees_unit(2, 0));
    }

    #[test]
    fn the_setting_decides_what_starts_explored_or_is_never_hidden() {
        let w = sighted(
            vec![soldier(vec![]), soldier(vec![])],
            6.0,
            3.0,
            aaa_map::FogSetting::Explored,
        );
        assert_eq!(w.seen(1, [10.0, 10.0]), Seen::Visible);
        assert_eq!(w.seen(1, [70.0, 70.0]), Seen::Explored);
        let w = sighted(
            vec![soldier(vec![]), soldier(vec![])],
            6.0,
            3.0,
            aaa_map::FogSetting::None,
        );
        assert_eq!(w.seen(2, [70.0, 70.0]), Seen::Visible);
        assert!(w.sees_unit(2, 0));
    }

    #[test]
    fn what_fights_a_side_is_shown_it_without_exploring() {
        // Too short-sighted to see each other 10 apart, but firing on each
        // other shows each to the other.
        let mut w = sighted(
            vec![
                soldier(vec![rifle(12.0, 1.0)]),
                soldier(vec![rifle(12.0, 1.0)]),
            ],
            2.0,
            3.0,
            aaa_map::FogSetting::Full,
        );
        assert!(!w.sees_unit(2, 0));
        w.tick();
        assert!(w.sees_unit(2, 0) && w.sees_unit(1, 1));
        // Once they stop, it's as if never seen.
        for u in &mut w.units {
            u.fighter = None;
            u.target = None;
        }
        w.regiments.iter_mut().for_each(|r| r.engaged.clear());
        w.tick();
        assert_eq!(w.seen(2, [10.0, 10.0]), Seen::Unexplored);
    }

    #[test]
    fn units_stray_only_from_a_zone_of_control() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        put(&mut w, 1, [60.0, 60.0]);
        let from = w.units[0].position;
        assert!(!w.strays(0, 1, from));
        w.regiments[0].control_range = 10.0;
        assert!(w.strays(0, 1, from) && !w.strays(0, 0, from));
    }

    /// Forest, whose first property is for infantry and whose second for
    /// vehicles, and the rules those properties work by.
    fn forest() -> (aaa_map::GroundKind, aaa_map::TerrainRules) {
        let forest = aaa_map::GroundKind {
            properties: vec![Id::from("forest_infantry"), Id::from("forest_vehicles")],
            mountain: false,
        };
        let infantry = TerrainEffect {
            requires: vec![Id::from("infantry")],
            defense: 4.0,
            resists: vec![(Id::from("gunfire"), 0.5)],
            speed_limit: Some(1.5),
        };
        let vehicles = TerrainEffect {
            requires: vec![Id::from("mobile"), Id::from("vehicle")],
            defense: 0.0,
            resists: vec![(Id::from("gunfire"), -0.2)],
            speed_limit: Some(1.0),
        };
        let rules = aaa_map::TerrainRules {
            effects: HashMap::from([
                (Id::from("forest_infantry"), infantry),
                (Id::from("forest_vehicles"), vehicles),
            ]),
            unlimited: vec![Id::from("trailblazing")],
            ..aaa_map::TerrainRules::default()
        };
        (forest, rules)
    }

    fn kind(properties: &[&str]) -> aaa_map::Kind {
        aaa_map::Kind {
            properties: properties.iter().map(|&p| Id::from(p)).collect(),
            ..aaa_map::Kind::default()
        }
    }

    #[test]
    fn terrain_holds_everyone_on_it_to_its_last_propertys_speed_limit() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        let (forest, rules) = forest();
        (w.grounds, w.terrain_rules) = (aaa_map::Grounds::uniform(20, forest), rules);
        w.kinds.insert(Id::from("soldier"), kind(&["infantry"]));
        let to = [10.0, 30.0];
        w.apply(7, Command::Move { regiment: 0, to }).unwrap();
        run(&mut w, 1.0);
        // The vehicles' limit, though they're infantry.
        assert!(w.units.iter().all(|u| u.speed == 1.0));
        // Never below the lowest there is.
        w.terrain_rules.slowest = 1.25;
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 1.25));
        // A mountain's is the lowest there is.
        let mountain = aaa_map::GroundKind {
            properties: Vec::new(),
            mountain: true,
        };
        (w.grounds, w.terrain_rules.slowest) = (aaa_map::Grounds::uniform(20, mountain), 0.5);
        run(&mut w, 0.2);
        assert!(w.units.iter().all(|u| u.speed == 0.5));
        w.kinds.insert(Id::from("squad"), kind(&["trailblazing"]));
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 2.0));
    }

    #[test]
    fn members_share_their_regiments_terrain_unless_they_straggle() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        let (forest, rules) = forest();
        w.terrain_rules = rules;
        w.grounds = aaa_map::Grounds::uniform(20, aaa_map::GroundKind::default());
        w.kinds.insert(Id::from("soldier"), kind(&["infantry"]));
        // Under the first only, who with no control range is on his own.
        w.grounds.set(tile_at(w.units[0].position), forest.clone());
        w.stand();
        let held = |w: &World, u| w.terrain_effects(Target::Unit(u)).len();
        assert_eq!((held(&w, 0), held(&w, 1)), (1, 0));
        assert!(w.regiments[0].terrain_properties.is_empty());
        assert_eq!((w.speed_limit(0), w.speed_limit(1)), (Some(1.0), None));
        w.regiments[0].control_range = 10.0;
        w.stand();
        assert_eq!((held(&w, 0), w.speed_limit(0)), (0, None));
        // Under the regiment, which gains only what a member can.
        w.grounds.set(tile_at(w.leader_position(0)), forest);
        w.stand();
        assert_eq!((held(&w, 0), held(&w, 1)), (1, 1));
        assert_eq!(
            w.regiments[0].terrain_properties,
            [Id::from("forest_infantry")]
        );
        // Unpacked, it keeps what it had wherever that leaves it.
        w.regiments[0].unpacked_into = Some(0);
        w.grounds = aaa_map::Grounds::uniform(20, aaa_map::GroundKind::default());
        w.stand();
        assert_eq!(
            w.regiments[0].terrain_properties,
            [Id::from("forest_infantry")]
        );
    }

    #[test]
    fn terrain_properties_add_to_defence_and_resistance() {
        let (_, rules) = forest();
        let effect = |id: &str| &rules.effects[&Id::from(id)];
        let hit =
            |ground: &[&TerrainEffect]| damage(&rifle(12.0, 12.0), &soldier(vec![]), ground, 1.0);
        assert_eq!(hit(&[]), 12.0);
        assert!((hit(&[effect("forest_infantry")]) - 4.0).abs() < 1e-5);
        assert!((hit(&[effect("forest_vehicles")]) - 14.4).abs() < 1e-5);
        let both = [effect("forest_infantry"), effect("forest_vehicles")];
        assert!((hit(&both) - 5.6).abs() < 1e-5);
    }
}
