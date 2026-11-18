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
mod companies;
mod computer;
mod delivery;
mod disposal;
pub mod economy;
mod efficiency;
mod engagement;
mod experience;
mod fog;
mod guard;
mod hash;
mod heroes;
mod morale;
mod movement;
mod notices;
mod objectives;
mod outcome;
mod packing;
pub mod path;
mod production;
mod properties;
mod remains;
mod research;
mod rules;
mod schedule;
mod setup;
mod strikes;
mod summons;
mod supply;
mod targets;
mod terrain;
#[cfg(test)]
mod testing;
mod triggers;
mod value;

use std::collections::{BTreeMap, VecDeque};
use std::f32::consts::{PI, TAU};

use tgmap::{
    DeployPoints, Domain, Fighter, Id, Obstacle, Pathing, Shape, TerrainIndex, Tile, Turret,
    Unpacks, Weapon,
};

pub use attach::Hq;
pub use combat::Struck;
pub use computer::{ComputerPlayer, Idle};
pub use delivery::{Delivery, Descent};
pub use economy::{Balance, Change, Ledger, Limit, Treasury};
pub use experience::{Commander, Operated, SpecialOp};
pub use fog::{Fog, Seen};
use hash::{Digest, Hasher, digest};
pub use heroes::Pooled;
pub use morale::Mind;
pub use notices::Notice;
pub use objectives::{Objective, Objectives, Standing};
pub use outcome::Fate;
pub use packing::unpack_site;
pub use properties::Held;
pub use remains::Remains;
pub use research::{Lacking, Researchable};
pub use rules::Rules;
pub use schedule::{
    Action, CHAT_LONGEST, Channel, Diverged, Given, Pausing, Record, SPEEDS, Schedule, Votes,
};
use serde::{Deserialize, Serialize};
pub use strikes::{Afflicted, Incoming, Watch};
pub use summons::Summoned;
pub use supply::{Bombarder, Supplying};
pub use tgmap::{Outcome, Player, Relation, Team};
pub use triggers::{Switch, Triggers};
pub use value::CombatMeans;

/// Seconds of play one [`World::tick`] advances.
pub const TICK: f32 = 0.1;

/// Ticks to a second of play.
pub(crate) const SECOND: u64 = (1.0 / TICK) as u64;

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

/// The share of an organization's control range its pursued enemy has to come
/// within for it to engage.
pub const PURSUIT_REACH: f32 = 0.6;

/// How far, squared, the enemy an organization pursues gets from where it's
/// headed, against how far, squared, it is from the organization, before the
/// organization turns after it: about 22.5° off course.
pub const PURSUIT_DRIFT: f32 = 0.14646;

/// A match in progress.
#[derive(Clone)]
pub struct World {
    pub sides: Sides,
    pub pathing: Pathing,
    /// What the match is played by: see [`World::rules`].
    pub(crate) rules: Rules,
    /// The terrain under each tile, for what it does to those on it.
    pub tile_terrains: tgmap::TileTerrains,
    pub organizations: Vec<Organization>,
    pub units: Vec<Unit>,
    pub buildings: Vec<Building>,
    /// The aircraft bringing organizations in, and those that have gone.
    pub deliveries: Vec<Delivery>,
    /// The obstacles that aren't buildings, such as trees and rocks.
    pub scenery: Vec<Obstacle>,
    /// The scenery still standing that a building going up clears away,
    /// as [`tgmap::Map::clearable`] has it.
    pub clearable: Vec<(u32, Obstacle)>,
    /// The ids of the scenery buildings have cleared away, in the order
    /// they went.
    pub cleared: Vec<u32>,
    /// The technologies each side has, by player id.
    pub technologies: BTreeMap<u32, Vec<Id>>,
    /// The objectives each side holds, by player id.
    pub objectives: BTreeMap<u32, Objectives>,
    /// The heroes each side holds, by player id.
    pub pools: BTreeMap<u32, Vec<heroes::Pooled>>,
    /// The map's triggers, and where each stands.
    pub triggers: Triggers,
    /// Where each side stands in the match, by player id.
    pub fates: BTreeMap<u32, Fate>,
    /// Whether the match is over: every side's outcome is decided, though
    /// play carries on.
    pub over: bool,
    /// What sides own that's neither an organization nor a building but may
    /// still add to their economy, such as where they start: each owner
    /// and template.
    pub holdings: Vec<(u32, Id)>,
    /// The settlements, which stay for as long as their centres do.
    pub settlements: Vec<tgmap::Settlement>,
    /// Each side's money, by player id.
    pub treasuries: BTreeMap<u32, Treasury>,
    /// The experience of each side with a faction, by player id.
    pub commanders: BTreeMap<u32, Commander>,
    /// The special operations carried out in the last [`LANDINGS_KEPT`]
    /// ticks, oldest first, for what shows them to find.
    pub operated: Vec<Operated>,
    /// The tick each side whose minimap has its fog lifted has it until.
    pub reveals: BTreeMap<u32, u64>,
    /// Strikes on their way.
    pub incoming: Vec<Incoming>,
    /// Ground sides are shown for a while.
    pub watches: Vec<Watch>,
    /// Units hurt each second by what strikes left on them.
    pub afflicted: Vec<Afflicted>,
    /// What special operations have put on the map.
    pub summoned: Vec<Summoned>,
    /// Which tiles are of the map's main land, once first asked.
    pub(crate) main_land: std::sync::OnceLock<Vec<bool>>,
    /// What sides have been told: see [`World::take_notices`].
    pub(crate) notices: Vec<Notice>,
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
    /// The last number each side has given a numbered template, by side
    /// and template.
    numbers: BTreeMap<(u32, Id), u32>,
    /// An id no object has yet.
    next_id: u32,
    /// An id no commission has yet.
    next_commission: u32,
    /// What the next [`World::chance`] is drawn from.
    rng: u64,
}

#[derive(Clone)]
pub struct Organization {
    /// Its object's id in the map.
    pub id: u32,
    pub template: Id,
    /// The number its name is given, for one of a numbered template: see
    /// [`Rules::numbered`].
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
    pub formations: Vec<tgmap::Formation>,
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
    pub engaged: Vec<Piece>,
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
    /// organization's for supply.
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
    /// The last hit an enemy dealt any of its members.
    pub struck: Option<Struck>,
    /// Where each member stood from its leader when it last broke
    /// formation, on the map's own axes, kept while it has none.
    pub broken_places: Vec<[f32; 2]>,
    /// The properties it has for a time, such as an operation's.
    pub properties: Vec<Held>,
    /// The guard it's keeping, until it's given another order.
    pub guard: Option<Guard>,
    /// Whether it's still being dropped: some of it yet to be let go, or
    /// to land.
    pub arriving: bool,
    /// The terrain it's on, as [`tgmap::TileTerrains::get`] takes it.
    pub terrain: TerrainIndex,
    /// The properties that terrain gives it.
    pub terrain_properties: Vec<Id>,
    /// Its morale, from 0 up to `morale_max`.
    pub morale: f32,
    /// The most morale it can have; 0 for one with no morale.
    pub morale_max: f32,
    /// What its morale has it doing.
    pub mind: Mind,
    /// Whether an enemy organization was engaging it at its last look at
    /// its morale.
    engaged_before: bool,
    /// The experience it's earned, which never goes down.
    pub experience: f32,
    /// The experience level it's reached, by index into its template's
    /// levels.
    pub level: usize,
    /// Whether it was engaging an enemy at its last look at its
    /// experience.
    engaging_before: bool,
    /// How hard it hits now, as a share of full strength, easing towards
    /// what its properties set.
    pub efficiency: f32,
}

/// An organization's order to keep a post: it comes back to it after every
/// fight, and beside something goes after whoever is fighting that.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guard {
    /// What it guards, for one that isn't guarding a place.
    pub of: Option<Piece>,
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

impl Organization {
    /// Takes up `order` in place of whatever it was told before, which
    /// it forgets. What it's engaged with it stays engaged with.
    pub(crate) fn take_order(&mut self, order: Order) {
        self.order = order;
        self.guard = None;
        self.resume = None;
        self.unpack_at = None;
        self.unpack_again = None;
        self.rally = None;
    }

    /// Whether it's resting at the post it guards, which has it stand in
    /// its combat formation, aggressive, with a longer control range.
    pub fn guarding(&self) -> bool {
        self.engaged.is_empty() && self.guard.is_some_and(|g| g.step == Step::Arrived)
    }

    /// Its zone of control's radius now.
    pub fn control(&self, rules: &tgmap::GuardRules) -> f32 {
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
    /// attacking or guarding, never while running or routing, else as its
    /// formation is.
    pub fn aggressive(&self) -> bool {
        if self.routing() {
            return false;
        }
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

/// An organization or a building: what a side owns on the map, and so what a
/// organization engages or guards, what holds properties and what's destroyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Piece {
    /// Index into [`World::organizations`].
    Organization(usize),
    /// Index into [`World::buildings`].
    Building(usize),
}

/// How an organization goes where it's sent.
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
    /// Going after the enemy organization at this index into
    /// [`World::organizations`] wherever it goes, engaging it once near enough
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

#[derive(Clone)]
pub struct Unit {
    /// Its object's id in the map.
    pub id: u32,
    pub template: Id,
    /// Index into [`World::organizations`].
    pub organization: usize,
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
    /// Its template's top speed, before its properties change it.
    pub speed_max: f32,
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
    /// What its properties multiply its fighter's health by, as its health
    /// was last scaled to.
    pub health_factor: f32,
    /// Seconds since each of its fighter's weapons last fired; infinite
    /// for one that hasn't.
    fired_ago: Vec<f32>,
    /// What it last fired on.
    pub target: Option<Target>,
    /// The tick it last fired on, and the index of the weapon it fired.
    pub shot: Option<(u64, usize)>,
    /// The tick it died on.
    pub died: Option<u64>,
    /// While it's under siege, the tick that may end on.
    pub besieged: Option<u64>,
    /// The death it died, by its ids, which plays before its remains start
    /// lying.
    pub death: Option<Id>,
    /// The last hit an enemy dealt it.
    pub struck: Option<Struck>,
    /// How it left the map without dying, if it has.
    pub left: Option<Left>,
    /// Whether it's yet to join its organization, as a recruit's members are
    /// until supply brings them.
    pub awaited: bool,
    /// Its fall, for one coming down by parachute, which is awaited
    /// until it lands.
    pub descent: Option<Descent>,
    /// The terrain it's on, as [`tgmap::TileTerrains::get`] takes it: its
    /// organization's, or its own tile's while it straggles.
    pub terrain: TerrainIndex,
}

/// How a unit left the map without dying, and the tick it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Left {
    /// Into the building it unpacked into, which packing brings it back
    /// from.
    Unpacked(u64),
    /// For good.
    Disbanded(u64),
    /// Its place in its organization given to another, such as a hero
    /// joining as captain.
    Replaced(u64),
}

/// What a unit tows, dragged after the point it's coupled to so that it
/// trails round corners.
#[derive(Clone)]
pub struct Trailer {
    pub template: Id,
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
#[derive(Clone)]
pub struct Building {
    /// Its object's id in the map.
    pub id: u32,
    pub template: Id,
    /// The number of the organization it stands in, as
    /// [`Organization::number`], for one of a numbered template.
    pub number: Option<u32>,
    /// The owning [`Player::id`], which changes when it's captured.
    pub owner: u32,
    pub position: [f32; 2],
    /// The way its model's -y axis points, a unit vector.
    pub facing: [f32; 2],
    pub fighter: Fighter,
    pub health: f32,
    /// What its properties multiply its fighter's health by, as its health
    /// was last scaled to.
    pub health_factor: f32,
    /// Seconds since each of its fighter's weapons last fired; infinite
    /// for one that hasn't.
    fired_ago: Vec<f32>,
    /// What it last fired on.
    pub target: Option<Target>,
    /// For one that defends itself, what it's noticed, which is all it
    /// fires on. Never forgotten, so not a sign of a fight.
    pub noticed: Vec<Piece>,
    /// For one that defends itself, whether it's been woken by noticing
    /// something: until then it neither aims nor fires.
    pub awake: bool,
    /// What was in its zone when it last looked, so it can tell what's
    /// come in since.
    inside: BTreeMap<Target, Footing>,
    /// While it's under siege, the tick that may end on.
    pub besieged: Option<u64>,
    /// The sides that remember its remains under the fog, once they've
    /// been taken off the map, until they see the place again.
    pub remembered: Vec<u32>,
    /// The last hit an enemy dealt it.
    pub struck: Option<Struck>,
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
    pub organization: Option<Box<tgmap::Organization>>,
    /// Where what it commissions moves on to once out, if set.
    pub rally: Option<[f32; 2]>,
    /// The tick it last changed hands on.
    pub captured: Option<u64>,
    /// For one unpacked during the match, the organization it unpacked from,
    /// by index into [`World::organizations`], which packing brings back.
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
    /// Packed up and gone, since the tick it packed on, into the organization
    /// at this index into [`World::organizations`].
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
    /// Where it's to be delivered, for one that's flown there rather
    /// than put out by its building.
    pub to: Option<[f32; 2]>,
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
    /// An organization, standing anywhere: it's placed and given ids once it's
    /// done.
    Raise(Box<tgmap::Organization>),
    Research(tgmap::Research),
}

impl Commission {
    fn of(work: Work, seconds: f32) -> Self {
        Commission {
            held: matches!(work, Work::Research(_)),
            work,
            ticks: ((seconds / TICK).round() as u64).max(1),
            paid: 0.0,
            to: None,
            id: 0,
        }
    }

    /// The organization it raises, if it raises one.
    pub fn organization(&self) -> Option<&tgmap::Organization> {
        match &self.work {
            Work::Raise(r) => Some(r),
            Work::Research(_) => None,
        }
    }

    /// What it's for: the template it raises or the technology it
    /// researches.
    pub fn template(&self) -> &Id {
        match &self.work {
            Work::Raise(r) => &r.template,
            Work::Research(r) => &r.technology,
        }
    }
}

/// Why a building's queue is held up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pause {
    /// Its owner hasn't room under a limit, such as `unit_limit_consumed`,
    /// for the first of the queue.
    Limit(Id),
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
    /// Its owner hasn't the money for it, or for a special operation the
    /// experience.
    Unaffordable,
    /// The HQ that would raise it has no place left for it.
    Full,
    /// It's a special operation that hasn't recharged since it was last
    /// used.
    Recharging,
    /// It's a special operation waiting on what it, or the like of it,
    /// put on the map.
    Summoned,
    /// It's a special operation that can't be aimed there.
    Misplaced,
    /// What would recruit it is under siege.
    Besieged,
    /// It would take its owner over a limit, such as
    /// `unit_limit_consumed`.
    OverLimit,
    /// It's a company that can't take or let go of a hero while it's
    /// marked out of supply.
    Unsettled,
    /// It's a company out of supply, which can't take or let go of a hero.
    OutOfSupply,
    /// It's a company too small to take or let go of a hero.
    Understrength,
    /// It's a hero that's dead, until it's healed in its side's pool.
    Dead,
    /// It's an organization routing, which takes no orders until it's
    /// done.
    Routing,
    /// It's an organization out of morale, which takes no orders until
    /// it's regained some.
    Exhausted,
}

impl From<NotYours> for Refused {
    fn from(_: NotYours) -> Self {
        Refused::NotYours
    }
}

/// An order a player gives, as data, so that every copy of a match can
/// play the same orders out on the same tick. Organizations and buildings are
/// by index into [`World::organizations`] and [`World::buildings`], which only
/// ever grow, so the same in every copy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Sends an organization to `to`, turned to face the way it's going, each
    /// member making for its place in the formation there. An engaged
    /// organization only sets off once it's done.
    Move { organization: usize, to: [f32; 2] },
    /// As [`Command::Move`], but in its combat formation and taking on
    /// every enemy that comes within its control range on the way.
    Attack { organization: usize, to: [f32; 2] },
    /// As [`Command::Move`], but breaking off any engagement and engaging
    /// nothing on the way.
    Run { organization: usize, to: [f32; 2] },
    /// Sends an organization after the enemy organization `enemy`, breaking off
    /// any engagement, to engage it once near enough. Refused for an
    /// organization with nothing to fire, or what isn't an enemy's.
    Pursue { organization: usize, enemy: usize },
    /// Posts an organization at `to`, which it goes to as on [`Command::Move`]
    /// and comes back to after every fight, until it's given another
    /// order. Resting there it's as [`Organization::guarding`] has it.
    Guard { organization: usize, to: [f32; 2] },
    /// As [`Command::Guard`], but posted beside `piece`, following it and
    /// going after the enemy organizations fighting it, until it's gone.
    /// Refused for the organization itself, or what's already gone.
    GuardPiece { organization: usize, piece: Piece },
    /// Has an organization rout at once, as if its morale had broken.
    /// Refused for one with no morale, that isn't fighting or can't move.
    Rout { organization: usize },
    /// Halts an organization, its members forming up around where they stand on
    /// average, and drops where it was headed, so that once done with any
    /// engagement it stays put.
    Stop { organization: usize },
    /// Sends a packed building to unpack at [`unpack_site`] for `at`, the
    /// building facing `facing`. Refused where it can't unpack or the
    /// building wouldn't fit.
    Unpack {
        organization: usize,
        at: [f32; 2],
        facing: [f32; 2],
    },
    /// Switches an organization to another of its formations, its members
    /// taking their new places where it stands, or is headed.
    Formation {
        organization: usize,
        formation: usize,
    },
    /// Starts a building packing into what it packs into, such as a
    /// truck, dropping whatever it was asked to commission. Refused while
    /// it's unpacking or packing, or too hurt to pack.
    Pack { building: usize },
    /// Asks a building to commission an organization of `template` once it's
    /// done with what it was asked for before. Refused for one it doesn't
    /// recruit.
    Commission { building: usize, template: Id },
    /// Recruits a company of `template` at the settlement at this index
    /// into [`World::settlements`], at once and paid for there and then.
    /// Refused for one the settlement doesn't list or lacks what it needs,
    /// while its centre is under siege, and for one its side can't pay for
    /// or hasn't room for under its limits. With a `hero`, the company
    /// is led by that hero from its side's pool in place of its own
    /// captain, and is refused unless the hero is
    /// [available](World::hero_available).
    Recruit {
        settlement: usize,
        template: Id,
        #[serde(default)]
        hero: Option<Id>,
    },
    /// As [`Command::Commission`], for an organization that's flown in and
    /// dropped at `to`. Refused for one nothing flies, or where it
    /// hasn't [room to land](World::room_to_land).
    Deliver {
        building: usize,
        template: Id,
        to: [f32; 2],
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
    /// Attaches an organization to an HQ with a place for its kind: the nearest
    /// for one that's detached, else the next after its own, round to the
    /// first again. Refused with none to go to.
    Attach { organization: usize },
    /// Detaches an organization from its HQ.
    Detach { organization: usize },
    /// Puts `hero`, from its side's pool, in a company's captain's place,
    /// taking it from any company it leads. Refused while either company
    /// can't [change captains](World::may_change_captain), and for a hero
    /// that's dead.
    AttachHero { organization: usize, hero: Id },
    /// Sends the hero leading a company back to its side's pool. Refused
    /// while the company can't [change captains](World::may_change_captain).
    DetachHero { organization: usize },
    /// Takes an organization, or a lone unit such as a depot's truck, off the
    /// map for good, for nothing. Refused for an organization that's engaged or
    /// attached to an HQ, or anything on its way to unpack.
    Disband { organization: usize },
    /// Takes a building off the map for its [`World::sale_value`].
    /// Refused for one whose template can't be sold, one unpacking or
    /// packing, and one under siege where the rules say so.
    Sell { building: usize },
    /// Uses the `operation`th of the player's faction's special
    /// operations, aimed at `at`, or given on `on` there. Refused while
    /// it's recharging, when the side hasn't the experience for it, and for
    /// one the match can't carry out.
    Operate {
        operation: usize,
        at: [f32; 2],
        on: Option<Piece>,
    },
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
        let spot = tgmap::from_local(self.position, self.facing, [-left, ahead]);
        let [right, ahead] = tgmap::to_local(spot, self.facing, at);
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    /// Index into [`World::units`].
    Unit(usize),
    /// Index into [`World::buildings`].
    Building(usize),
}

impl World {
    /// Where an organization's surviving members stand on average, or, with
    /// none on the ground yet, where it does.
    pub fn centre(&self, organization: usize) -> [f32; 2] {
        let survivors: Vec<usize> = self.survivors(organization).collect();
        match survivors.is_empty() {
            true => self.organizations[organization].position,
            false => self.average(&survivors),
        }
    }

    /// An organization's members still alive.
    pub fn survivors(&self, organization: usize) -> impl Iterator<Item = usize> + '_ {
        let units = &self.organizations[organization].units;
        units.iter().copied().filter(|&u| self.units[u].alive())
    }

    /// Where an organization is headed: where it stands once there, or, while
    /// fighting on the way, where it carries on to after.
    pub fn destination(&self, organization: usize) -> [f32; 2] {
        let r = &self.organizations[organization];
        r.resume.map_or(r.position, |(to, _)| to)
    }

    /// Whether an organization is still on its way somewhere: moving, or
    /// fighting before it carries on.
    pub fn underway(&self, organization: usize) -> bool {
        let r = &self.organizations[organization];
        r.resume.is_some() || self.survivors(organization).any(|u| self.units[u].moving())
    }

    /// Whether an organization has lost every member, which one still being
    /// dropped hasn't.
    pub fn destroyed(&self, organization: usize) -> bool {
        !self.organizations[organization].arriving && self.survivors(organization).next().is_none()
    }

    fn average(&self, units: &[usize]) -> [f32; 2] {
        let n = units.len().max(1) as f32;
        let [x, y] = units.iter().fold([0.0, 0.0], |[x, y], &u| {
            let [ux, uy] = self.units[u].position;
            [x + ux, y + uy]
        });
        [x / n, y / n]
    }

    /// Kills an organization's surviving members outright.
    pub fn destroy(&mut self, organization: usize) {
        for u in self.organizations[organization].units.clone() {
            let unit = &mut self.units[u];
            if unit.alive() {
                unit.die(self.ticks);
                self.mourn(u);
                self.lost_member(u);
            }
        }
    }

    /// Tells of the organization of the unit at `u`, just killed, if it has
    /// none left. A packed HQ whose truck is gone loses its organizations.
    fn lost_member(&mut self, u: usize) {
        let r = self.units[u].organization;
        if let Some(hq) = self.truckless_hq(r) {
            self.detach_all(hq);
        }
        if self.destroyed(r) {
            self.notices.push(Notice::Destroyed {
                owner: self.organizations[r].owner,
                what: Piece::Organization(r),
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
        self.keep_morale();
        self.keep_efficiency();
        self.keep_experience();
        self.keep_health();
        self.keep_guard();
        self.close_in();
        self.occupy();
        self.decay();
        self.lift_sieges();
        self.supply();
        self.resupply();
        self.tend_heroes();
        self.produce();
        self.deliver();
        self.rally();
        self.unpack();
        self.earn();
        self.lead_sides();
        self.look();
        self.ticks += 1;
        self.clear_remains();
        self.run_triggers();
        self.settle_outcomes();
        self.clear_out_resigned();
    }
}

/// Where a deployment offset from a building standing at `position`
/// facing `facing` lies.
fn deploy_point(position: [f32; 2], facing: [f32; 2], offset: [f32; 2]) -> [f32; 2] {
    tgmap::from_local(position, facing, offset)
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

/// An order to an organization from a player that doesn't own it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotYours;

/// The players in a match and how they stand toward each other.
#[derive(Debug, Clone, Default)]
pub struct Sides {
    pub players: Vec<Player>,
    pub teams: Vec<Team>,
    /// What each side a person plays goes by, by player id: the person's
    /// own name, where they have one in the match.
    pub names: BTreeMap<u32, String>,
    /// What each side a computer player took over from a person goes by.
    pub computers: BTreeMap<u32, String>,
}

impl Sides {
    pub fn player(&self, id: u32) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }

    /// How the player `from` stands toward the player `to`, by their ids;
    /// neutral toward a player the match doesn't have.
    pub fn relation(&self, from: u32, to: u32) -> Relation {
        tgmap::relation(&self.players, &self.teams, from, to)
    }

    /// The player a person plays by default: the first a person may.
    pub fn first_playable(&self) -> Option<&Player> {
        self.players.iter().find(|p| p.playable && !p.independent)
    }
}

digest!(World {
    organizations, units, buildings, deliveries, cleared, technologies, objectives, fates, over,
    holdings, settlements, treasuries, commanders, pools, triggers, operated, reveals, incoming, watches, afflicted, summoned, fog, ticks, loaded, flying, landings, numbers, next_id,
    next_commission, rng, sides,
} skip {
    // Fixed by the map and the game's data.
    rules, tile_terrains, scenery,
    // Follow from the buildings standing and the scenery cleared, or from
    // the terrain alone.
    pathing, clearable, main_land,
    // Taken by whatever shows them, whenever it does.
    notices,
});

digest!(Sides { names, computers } skip {
    // Fixed by the map.
    players, teams,
});

digest!(Organization {
    id, template, number, owner, units, speed, position, facing, formation, combat,
    operational_range, control_range, order, engaged, resume, unpack_at, unpack_again,
    unpacked_into, rally, leader, hq, in_supply, replacing, bombarding, bombarded, properties,
    guard, arriving, terrain_properties, morale, morale_max, mind, engaged_before, experience,
    level, engaging_before, efficiency, struck, broken_places,
} skip {
    // Its template's.
    formations, unpacks,
    // Follows from where it stands.
    terrain,
});

digest!(Guard {
    of,
    post,
    facing,
    step,
    unreachable
});

digest!(Leg {
    heading,
    aim,
    snapped
});

digest!(Unit {
    id, template, organization, position, previous, facing, end_facing, path, from, leg, goal,
    exact, ways, look, speed, acceleration, turn_rate, turn_radius, trailer, closing, health,
    health_factor,
    fired_ago, target, shot, died, death, besieged, struck, left, awaited, descent,
} skip {
    // Its template's.
    speed_max,
    domain, shape, fighter,
    // Follows from where it stands.
    terrain,
});

digest!(Trailer {
    template,
    position,
    previous,
    facing,
    hitch,
    coupling
});

digest!(Building {
    id, template, number, owner, position, facing, health, health_factor, fired_ago, target,
    noticed, awake,
    inside, besieged, struck, turrets, shot, state, unpacking, packing, queue, progress, paused, rally,
    captured, unpacked_from, zone, members, supply, properties, created, remembered,
} skip {
    // Its template's, or as the map has it.
    fighter, deploy, organization, obstacle,
});

digest!(Commission {
    work,
    ticks,
    paid,
    to,
    held,
    id
});

digest!(TurretAim { yaw, idle });

digest!(Projectile { from, by, weapon, target, at, lands, bombard } skip {
    // Its firer's template's.
    shot,
});

digest!(Landing {
    tick,
    from,
    weapon,
    target,
    at
});

digest!(tgmap::Settlement {
    id,
    template,
    owner,
    centre
});

impl Digest for Step {
    fn digest(&self, h: &mut Hasher) {
        (*self as u8).digest(h);
    }
}

impl Digest for Piece {
    fn digest(&self, h: &mut Hasher) {
        match *self {
            Piece::Organization(r) => (0u8, r).digest(h),
            Piece::Building(b) => (1u8, b).digest(h),
        }
    }
}

impl Digest for Target {
    fn digest(&self, h: &mut Hasher) {
        match *self {
            Target::Unit(u) => (0u8, u).digest(h),
            Target::Building(b) => (1u8, b).digest(h),
        }
    }
}

impl Digest for Order {
    fn digest(&self, h: &mut Hasher) {
        match *self {
            Order::Move => 0u8.digest(h),
            Order::Run => 1u8.digest(h),
            Order::Attack => 2u8.digest(h),
            Order::Pursue(r) => (3u8, r).digest(h),
        }
    }
}

impl Digest for Left {
    fn digest(&self, h: &mut Hasher) {
        match *self {
            Left::Unpacked(tick) => (0u8, tick).digest(h),
            Left::Disbanded(tick) => (1u8, tick).digest(h),
            Left::Replaced(tick) => (2u8, tick).digest(h),
        }
    }
}

impl Digest for State {
    fn digest(&self, h: &mut Hasher) {
        match *self {
            State::Standing => 0u8.digest(h),
            State::Disabled => 1u8.digest(h),
            State::Destroyed(tick) => (2u8, tick).digest(h),
            State::Packed(tick, r) => (3u8, (tick, r)).digest(h),
            State::Sold(tick) => (4u8, tick).digest(h),
        }
    }
}

impl Digest for Work {
    /// By what it's for: the rest is that template's or technology's.
    fn digest(&self, h: &mut Hasher) {
        match self {
            Work::Raise(r) => (0u8, &r.template).digest(h),
            Work::Research(r) => (1u8, &r.technology).digest(h),
        }
    }
}

impl Digest for Pause {
    fn digest(&self, h: &mut Hasher) {
        match self {
            Pause::Limit(limit) => (0u8, limit.as_str()).digest(h),
            Pause::Disabled => 1u8.digest(h),
            Pause::Supply => 2u8.digest(h),
        }
    }
}

impl Digest for Outcome {
    fn digest(&self, h: &mut Hasher) {
        (*self as u8).digest(h);
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
        let organizations: Vec<_> = w
            .organizations
            .iter()
            .map(|r| (r.position, r.facing, r.formation, r.order, &r.engaged))
            .collect();
        let buildings: Vec<_> = w
            .buildings
            .iter()
            .map(|b| (b.owner, b.health, b.target, b.state, b.captured))
            .collect();
        format!("{:?}", (w.ticks, units, organizations, buildings))
    }

    /// Four organizations of riflemen moving, running and fighting around a
    /// house, played out to the end.
    fn played_battle() -> World {
        let rifles = |n| (0..n).map(|_| soldier(vec![rifle(12.0, 25.0)])).collect();
        let mut w = battle(&[1, 2, 3, 2], rifles(4), at_war());
        let house = building(20, 2, [30.0, 10.0]);
        w.pathing.block(house.obstacle.as_ref().unwrap());
        w.add_buildings(&[house]);
        let to = [30.0, 30.0];
        w.apply(
            1,
            Command::Move {
                organization: 0,
                to,
            },
        )
        .unwrap();
        let to = [10.0, 50.0];
        w.apply(
            3,
            Command::Run {
                organization: 2,
                to,
            },
        )
        .unwrap();
        run(&mut w, 15.0);
        let to = [20.0, 20.0];
        w.apply(
            2,
            Command::Move {
                organization: 3,
                to,
            },
        )
        .unwrap();
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
            for r in &w.organizations {
                numbers.extend(r.position.iter().chain(&r.facing).map(f));
                numbers.push(r.formation as u64);
            }
            for b in &w.buildings {
                numbers.extend([u64::from(b.owner), f(&b.health)]);
                numbers.push(b.captured.unwrap_or(u64::MAX));
            }
        }
        let mut h = Hasher::new();
        numbers.iter().for_each(|n| n.digest(&mut h));
        h.finish()
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
            names: Default::default(),
            computers: Default::default(),
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
            names: Default::default(),
            computers: Default::default(),
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
    fn copies_given_the_same_orders_hash_the_same() {
        let (a, b) = (played_battle(), played_battle());
        assert_eq!(a.hash(), b.hash());
        // Hashing changes nothing, and a tick does.
        assert_eq!(a.hash(), a.hash());
        let mut c = played_battle();
        c.tick();
        assert_ne!(a.hash(), c.hash());
    }

    #[test]
    fn an_order_only_one_copy_was_given_shows_in_its_hash() {
        let (a, mut b) = (played_battle(), played_battle());
        let to = [40.0, 40.0];
        let given = (0..b.organizations.len()).any(|organization| {
            let owner = b.organizations[organization].owner;
            b.apply(owner, Command::Move { organization, to }).is_ok()
        });
        assert!(given);
        assert_ne!(a.hash(), b.hash());
    }

    #[test]
    fn what_is_drawn_by_chance_shows_in_the_hash() {
        let (a, mut b) = (played_battle(), played_battle());
        b.chance();
        assert_ne!(a.hash(), b.hash());
    }

    #[test]
    fn what_sides_have_and_see_shows_in_the_hash() {
        let a = played_battle().hash();
        let differs = |change: fn(&mut World)| {
            let mut b = played_battle();
            change(&mut b);
            a != b.hash()
        };
        assert!(differs(
            |w| w.treasuries.entry(1).or_default().stockpile += 1.0
        ));
        assert!(differs(|w| w.grant(1, Id::from("radar"))));
        assert!(differs(|w| w.set_fog(tgmap::FogSetting::Full)));
        assert!(differs(
            |w| w.fates.get_mut(&1).unwrap().default_defeat ^= true
        ));
        assert!(differs(|w| w.organizations[0].in_supply ^= true));
        assert!(differs(|w| w.buildings[0].awake ^= true));
    }

    #[test]
    fn a_number_that_is_none_hashes_the_same_whatever_its_bits() {
        let hash = |x: f32| {
            let mut h = Hasher::new();
            x.digest(&mut h);
            h.finish()
        };
        let other = f32::from_bits(f32::NAN.to_bits() ^ 0x8000_0001);
        assert!(other.is_nan());
        assert_eq!(hash(f32::NAN), hash(other));
        assert_ne!(hash(0.0), hash(-0.0));
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
        let trailer = tgmap::Trailer {
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
            towing
                .apply(
                    7,
                    Command::Move {
                        organization: 0,
                        to,
                    },
                )
                .unwrap();
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
        let map = include_str!("../../tgmap/clippy.toml");
        assert_eq!(include_str!("../clippy.toml"), map);
    }
}
