//! The rules of the game, played out with no rendering so that it can run
//! headless.
//!
//! A *match* is one play of a map: a *match setup* (the map, its players and
//! their nations, teams and rules) once it's running. Each *player* is a side
//! in it, controlled by a person or an AI.

mod attach;
mod capture;
mod combat;
mod commands;
mod disposal;
pub mod economy;
mod engagement;
mod experience;
mod fog;
mod guard;
mod movement;
mod objectives;
mod packing;
pub mod path;
mod production;
mod research;
mod setup;
mod supply;
mod terrain;
#[cfg(test)]
mod testing;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::f32::consts::{PI, TAU};

use aaa_map::{
    DeployPoints, Domain, Fighter, Id, Obstacle, Packs, Pathing, Requires, Shape, TerrainIndex,
    Tile, Turret, Unpacks, Weapon,
};

pub use aaa_map::{Player, Relation, Team};
pub use attach::Hq;
pub use economy::{Balance, Change, Fallen, Ledger, Limit, Notice, Treasury};
pub use experience::Commander;
pub use fog::{Fog, Seen};
pub use objectives::{Objective, Objectives, Standing};
pub use packing::unpack_site;
pub use research::{Lacking, Researchable};
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

/// Ticks between a moving unit's looks at how its move is going, and
/// between those of one that's given up turning onto its leg. The game
/// spreads them over 1 to 1.3 seconds and 0.3 to 0.6.
const LOOK: u32 = 12;

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
    /// The terrain under each tile, for what it does to those on it.
    pub tile_terrains: aaa_map::TileTerrains,
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
    /// The building templates whose buildings their owner can sell.
    pub sellable: HashSet<Id>,
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
    /// The objectives each side holds, by player id.
    pub objectives: BTreeMap<u32, Objectives>,
    /// Whether those are kept in order of their text.
    pub sort_objectives: bool,
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
    /// What each template that researches lists, in its menu's order, by
    /// template id.
    pub research_lists: HashMap<Id, Vec<aaa_map::Research>>,
    /// What sides own that's neither a regiment nor a building but may
    /// still add to their economy, such as where they start: each owner
    /// and template.
    pub holdings: Vec<(u32, String)>,
    /// Each side's money, by player id.
    pub treasuries: BTreeMap<u32, Treasury>,
    /// The experience of each side with a general, by player id.
    pub commanders: BTreeMap<u32, Commander>,
    /// The experience every side with a general gains a second.
    pub experience_per_second: f32,
    /// What sides have been told since these were last taken.
    pub notices: Vec<Notice>,
    /// How far each template sees, by template.
    pub sights: HashMap<Id, aaa_map::Sight>,
    /// What each side sees of the map.
    pub fog: Fog,
    /// Ticks played.
    pub ticks: u64,
    /// Whether defending buildings have had their first look round, which
    /// stands for the map loading.
    loaded: bool,
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
    /// The terrain it's on, as [`aaa_map::TileTerrains::get`] takes it.
    pub terrain: TerrainIndex,
    /// The properties that terrain gives it.
    pub terrain_properties: Vec<Id>,
    /// `template`, as what's kept by template has it.
    template_id: Id,
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
    /// How it left the map without dying, if it has.
    pub left: Option<Left>,
    /// Whether it's yet to join its regiment, as a recruit's members are
    /// until supply brings them.
    pub awaited: bool,
    /// The terrain it's on, as [`aaa_map::TileTerrains::get`] takes it: its
    /// regiment's, or its own tile's while it straggles.
    pub terrain: TerrainIndex,
    /// `template`, as what's kept by template has it.
    template_id: Id,
}

/// How a unit left the map without dying, and the tick it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Left {
    /// Into the building it unpacked into, which packing brings it back
    /// from.
    Unpacked(u64),
    /// For good.
    Disbanded(u64),
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
    /// For one that defends itself, whether it's been woken by noticing
    /// something: until then it neither aims nor fires.
    pub awake: bool,
    /// What was in its zone when it last looked, so it can tell what's
    /// come in since.
    inside: HashMap<Target, Footing>,
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
    /// Sold and gone, since the tick it was sold on.
    Sold(u64),
}

/// Something a building has been asked to commission or research.
#[derive(Debug, Clone)]
pub struct Commission {
    pub work: Work,
    /// Ticks it takes.
    pub ticks: u64,
    /// The money taken for it, given back if it's called off.
    pub paid: f32,
    /// Whether it's taken its share of its owner's limits, which it does
    /// on reaching the front of the queue once there's room. Research
    /// takes none, so it's held from the start.
    pub held: bool,
    /// Set once a building takes it on, to call it off by: unlike its
    /// place in the queue, it stays the same while orders are on their way.
    pub id: u32,
}

/// What a building's queue works on.
#[derive(Debug, Clone)]
pub enum Work {
    /// A regiment, standing anywhere: it's placed and given ids once it's
    /// done.
    Raise(Box<aaa_map::Regiment>),
    Research(aaa_map::Research),
}

impl Commission {
    /// A commission raising `regiment` in `seconds`, at least a tick.
    pub fn new(regiment: aaa_map::Regiment, seconds: f32) -> Self {
        Commission::of(Work::Raise(Box::new(regiment)), seconds)
    }

    fn of(work: Work, seconds: f32) -> Self {
        Commission {
            held: matches!(work, Work::Research(_)),
            work,
            ticks: ((seconds / TICK).round() as u64).max(1),
            paid: 0.0,
            id: 0,
        }
    }

    /// The regiment it raises, if it raises one.
    pub fn regiment(&self) -> Option<&aaa_map::Regiment> {
        match &self.work {
            Work::Raise(r) => Some(r),
            Work::Research(_) => None,
        }
    }

    /// What it's for: the template it raises or the technology it
    /// researches.
    pub fn template(&self) -> &str {
        match &self.work {
            Work::Raise(r) => &r.template,
            Work::Research(r) => r.technology.as_str(),
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
    /// Asks a building to research `technology`, one of those it lists,
    /// once it's done with what it was asked for before. Refused for one
    /// it doesn't offer, as [`World::researchable`] has it.
    Research { building: usize, technology: Id },
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
    /// Takes a regiment, or a lone unit such as a depot's truck, off the
    /// map for good, for nothing. Refused for a regiment that's engaged or
    /// attached to an HQ, or anything on its way to unpack.
    Disband { regiment: usize },
    /// Takes a building off the map for its [`World::sale_value`].
    /// Refused for one whose template can't be sold, one unpacking or
    /// packing, and one under siege where the rules say so.
    Sell { building: usize },
}

impl Building {
    /// Whether it still takes part: it can be hit and fire.
    pub fn standing(&self) -> bool {
        self.state == State::Standing
    }

    /// Whether it's left the map, destroyed or packed up.
    pub fn gone(&self) -> bool {
        matches!(
            self.state,
            State::Destroyed(_) | State::Packed(..) | State::Sold(_)
        )
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

/// An angle in radians brought into -π to π.
fn wrap(angle: f32) -> f32 {
    angle - TAU * ((angle + PI) / TAU).floor()
}

/// The tile something stands on and whose side it's on.
type Footing = (Tile, u32);

/// Something a weapon can fire on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    /// Index into [`World::units`].
    Unit(usize),
    /// Index into [`World::buildings`].
    Building(usize),
}

impl World {
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

    /// A number from 0 up to 1, the same in every copy of a match.
    pub(crate) fn chance(&mut self) -> f32 {
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
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
        self.gain_experience();
        self.look();
        self.ticks += 1;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

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

    #[test]
    fn the_same_orders_play_out_the_same_way() {
        let (a, b) = (played_battle(), played_battle());
        assert!(a.units.iter().any(|u| u.died.is_some()));
        assert_eq!(snapshot(&a), snapshot(&b));
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
}
