//! The rules of the game, played out with no rendering so that it can run
//! headless.
//!
//! A *match* is one play of a map: a *match setup* (the map, its players and
//! their nations, teams and rules) once it's running. Each *player* is a side
//! in it, controlled by a person or an AI.

pub mod path;

use aaa_map::pathing::tile_at;
use aaa_map::{Domain, Fighter, Id, Map, Obstacle, Pathing, Weapon};

pub use aaa_map::{Player, Relation, Team};

/// Seconds of play one [`World::tick`] advances.
pub const TICK: f32 = 0.1;

/// The share of its health a building taken over by force is left with.
pub const CAPTURE_HEALTH: f32 = 0.3;

/// How near a moving unit comes to an enemy to run into it.
pub const CONTACT: f32 = 1.5;

/// A match in progress.
pub struct World {
    pub sides: Sides,
    pub pathing: Pathing,
    pub regiments: Vec<Regiment>,
    pub units: Vec<Unit>,
    pub buildings: Vec<Building>,
    /// Ticks played.
    pub ticks: u64,
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
    pub order: Order,
    /// The enemies it's engaged with. While there are any it holds its
    /// ground against them.
    pub engaged: Vec<Enemy>,
    /// Where it was headed and how it was to face there when it engaged,
    /// to carry on once it's done.
    resume: Option<([f32; 2], [f32; 2])>,
}

impl Regiment {
    /// Index into `formations` of the one it's in: its combat formation
    /// while engaged.
    pub fn current_formation(&self) -> usize {
        match self.engaged.is_empty() {
            true => self.formation,
            false => self.combat.unwrap_or(self.formation),
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
    /// As [`Order::Move`], and engaging any enemy its members can fire on
    /// too, until it gets there; then back to [`Order::Move`].
    Attack,
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
    /// The way to face once it arrives.
    end_facing: [f32; 2],
    /// Waypoints still ahead, the next last.
    path: Vec<[f32; 2]>,
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
}

impl Unit {
    pub fn moving(&self) -> bool {
        !self.path.is_empty()
    }

    pub fn alive(&self) -> bool {
        self.died.is_none()
    }

    fn die(&mut self, tick: u64) {
        self.health = 0.0;
        self.died = Some(tick);
        self.path.clear();
        self.target = None;
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
    pub fighter: Fighter,
    pub health: f32,
    /// Seconds until each of its fighter's weapons can fire again.
    reloads: Vec<f32>,
    /// What it last fired on.
    pub target: Option<Target>,
    /// The tick it last fired on, and the index of the weapon it fired.
    pub shot: Option<(u64, usize)>,
    pub state: State,
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
}

impl Building {
    /// Whether it still takes part: it can be hit and fire.
    pub fn standing(&self) -> bool {
        self.state == State::Standing
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
            ticks: 0,
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
                id: m.thing.id,
                template: m.thing.template.clone(),
                regiment: r,
                domain,
                position: [x, y],
                previous: [x, y],
                facing,
                end_facing: facing,
                path: Vec::new(),
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
            order: Order::Move,
            engaged: Vec::new(),
            resume: None,
        });
        r
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
            });
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
        self.regiments[regiment].owner == player && !self.destroyed(regiment)
    }

    /// Sends a regiment to `target`, turned to face the way it's going,
    /// each member making for its place in the formation there. An engaged
    /// regiment only sets off once it's done.
    pub fn order_move(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.advance(player, regiment, target, Order::Move)
    }

    /// Sends a regiment to `target` as [`World::order_move`] does, taking
    /// on every enemy it comes within reach of on the way.
    pub fn order_attack(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.advance(player, regiment, target, Order::Attack)
    }

    fn advance(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
        order: Order,
    ) -> Result<(), NotYours> {
        if !self.commands(player, regiment) {
            return Err(NotYours);
        }
        let r = &mut self.regiments[regiment];
        r.order = order;
        let from = r.position;
        let facing = normalised([target[0] - from[0], target[1] - from[1]]).unwrap_or(r.facing);
        match r.engaged.is_empty() {
            true => self.send(regiment, target, facing),
            false => r.resume = Some((target, facing)),
        }
        Ok(())
    }

    /// Sends a regiment to `target` as [`World::order_move`] does, but
    /// breaking off any engagement and engaging nothing on the way.
    pub fn order_run(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        if !self.commands(player, regiment) {
            return Err(NotYours);
        }
        let r = &mut self.regiments[regiment];
        r.order = Order::Run;
        r.engaged.clear();
        r.resume = None;
        let from = r.position;
        let facing = normalised([target[0] - from[0], target[1] - from[1]]).unwrap_or(r.facing);
        self.send(regiment, target, facing);
        Ok(())
    }

    /// Halts a regiment, its members forming up around where they stand on
    /// average, and drops where it was headed, so that once done with any
    /// engagement it stays put.
    pub fn order_stop(&mut self, player: u32, regiment: usize) -> Result<(), NotYours> {
        if !self.commands(player, regiment) {
            return Err(NotYours);
        }
        let here = self.centre(regiment);
        let r = &mut self.regiments[regiment];
        r.order = Order::Move;
        r.resume = None;
        if r.engaged.is_empty() {
            let facing = r.facing;
            self.send(regiment, here, facing);
        }
        Ok(())
    }

    fn send(&mut self, regiment: usize, target: [f32; 2], facing: [f32; 2]) {
        let r = &mut self.regiments[regiment];
        r.position = target;
        r.facing = facing;
        self.take_places(regiment);
    }

    /// Switches a regiment to another of its formations, its members
    /// taking their new places where it stands, or is headed.
    pub fn set_formation(
        &mut self,
        player: u32,
        regiment: usize,
        formation: usize,
    ) -> Result<(), NotYours> {
        if !self.commands(player, regiment) {
            return Err(NotYours);
        }
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
        let offsets = &r.formations[r.current_formation()].offsets;
        for (&u, &offset) in r.units.iter().zip(offsets) {
            let place = aaa_map::from_local(r.position, r.facing, offset);
            let unit = &mut self.units[u];
            if !unit.alive() {
                continue;
            }
            let mut way = path::find(&self.pathing, unit.domain, unit.position, place);
            way.reverse();
            unit.path = way;
            unit.end_facing = r.facing;
            if unit.path.is_empty() {
                unit.facing = r.facing;
            }
        }
    }

    /// Plays one [`TICK`].
    pub fn tick(&mut self) {
        self.travel();
        let fired_on = self.fight();
        self.engage(&fired_on);
        self.occupy();
        self.ticks += 1;
    }

    /// Ends the engagements whose enemies are gone, then engages regiments
    /// with what `fired_on` says fired on them, by regiment, where it's in
    /// their operational range, moving ones with enemy units they run
    /// into, and those on [`Order::Attack`] with enemies they can fire on.
    /// Regiments on [`Order::Run`] engage nothing.
    fn engage(&mut self, fired_on: &[(usize, Target)]) {
        for r in 0..self.regiments.len() {
            let regiment = &self.regiments[r];
            if regiment.order != Order::Move
                && regiment.engaged.is_empty()
                && !self.survivors(r).any(|u| self.units[u].moving())
            {
                self.regiments[r].order = Order::Move;
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
            if !unit.alive() || !unit.moving() {
                continue;
            }
            let met = self.units.iter().enumerate().find(|&(v, other)| {
                v != u
                    && other.alive()
                    && distance(unit.position, other.position) <= CONTACT
                    && self.hostile(unit.regiment, Enemy::Regiment(other.regiment))
            });
            if let Some((_, other)) = met {
                meetings.push((unit.regiment, Enemy::Regiment(other.regiment)));
            }
        }
        for r in 0..self.regiments.len() {
            if self.regiments[r].order == Order::Attack
                && let Some(enemy) = self.in_reach(r)
            {
                meetings.push((r, enemy));
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
    }

    /// The nearest enemy any of a regiment's members can fire on.
    fn in_reach(&self, r: usize) -> Option<Enemy> {
        let enemy = |t| match t {
            Target::Unit(u) => Enemy::Regiment(self.units[u].regiment),
            Target::Building(b) => Enemy::Building(b),
        };
        self.survivors(r)
            .filter_map(|u| Some((u, self.units[u].fighter.as_ref()?)))
            .flat_map(|(u, f)| {
                let s = Target::Unit(u);
                let from = self.units[u].position;
                f.weapons.iter().flat_map(move |w| {
                    self.targets()
                        .filter(move |&t| self.can_hit(s, w, t) && self.hostile(r, enemy(t)))
                        .map(move |t| (self.reach(from, t), t))
                })
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, t)| enemy(t))
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
                .any(|u| distance(here, self.units[u].position) <= reach),
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
        regiment.resume = Some((regiment.position, regiment.facing));
        let facing = normalised([there[0] - here[0], there[1] - here[1]]);
        let facing = facing.unwrap_or(regiment.facing);
        self.send(r, here, facing);
    }

    /// Sends a regiment that's no longer engaged on its way again, in the
    /// formation it was put in.
    fn disengage(&mut self, r: usize) {
        match self.regiments[r].resume.take() {
            Some((target, facing)) => self.send(r, target, facing),
            None => self.take_places(r),
        }
    }

    fn travel(&mut self) {
        for unit in &mut self.units {
            unit.previous = unit.position;
            let mut left = self.regiments[unit.regiment].speed * TICK;
            while let Some(&next) = unit.path.last() {
                let (dx, dy) = (next[0] - unit.position[0], next[1] - unit.position[1]);
                let distance = dx.hypot(dy);
                if let Some(f) = normalised([dx, dy]) {
                    unit.facing = f;
                }
                if distance > left {
                    unit.position[0] += dx / distance * left;
                    unit.position[1] += dy / distance * left;
                    break;
                }
                unit.position = next;
                left -= distance;
                unit.path.pop();
                if unit.path.is_empty() {
                    unit.facing = unit.end_facing;
                }
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
        }
    }

    /// What `s` fires `weapon` at: the best it can, armed enemies first,
    /// then what can be captured or destroyed, then the rest, and the
    /// nearest of them; or the one it fired on last as long as it's as good.
    fn aim(&self, s: Target, weapon: &Weapon, last: Option<Target>) -> Option<Target> {
        let from = self.position(s);
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
        let Some(fighter) = self.fighter(t).filter(|_| t != s) else {
            return false;
        };
        let d = self.reach(self.position(s), t);
        if d > weapon.range || d < weapon.min_range {
            return false;
        }
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
            let destroyed = matches!(self.buildings[c].state, State::Destroyed(_));
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

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
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
        if from == to {
            return Relation::Itself;
        }
        let index = |id| self.players.iter().position(|p| p.id == id);
        let (Some(a), Some(b)) = (index(from), index(to)) else {
            return Relation::Neutral;
        };
        let (pa, pb) = (&self.players[a], &self.players[b]);
        // Players put in a team keep their own stored relations at war
        // with everyone, so the team's speak for them.
        let stance = match (pa.team, pb.team) {
            (Some(ta), Some(tb)) => self.teams[ta].team_relations.get(tb),
            (Some(ta), None) => self.teams[ta].relations.get(b),
            (None, Some(tb)) => pa.team_relations.get(tb),
            (None, None) => pa.relations.get(b),
        };
        stance.copied().unwrap_or(Relation::Neutral)
    }

    /// The player a person plays by default: the first a person may.
    pub fn first_playable(&self) -> Option<&Player> {
        self.players.iter().find(|p| p.playable && !p.independent)
    }
}

fn normalised([x, y]: [f32; 2]) -> Option<[f32; 2]> {
    let length = x.hypot(y);
    (length > 1e-6).then(|| [x / length, y / length])
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
            fighter: None,
        }
    }

    /// Two soldiers side by side facing +y, the slower one on the right,
    /// who can also line up one behind the other.
    fn world(pathing: Pathing) -> World {
        let formation = |id: &str, offsets| aaa_map::Formation {
            id: id.into(),
            offsets,
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
            primary: None,
        };
        World::from_regiments(Sides::default(), pathing, &[regiment])
    }

    fn run(world: &mut World, seconds: f32) {
        for _ in 0..(seconds / TICK).round() as usize {
            world.tick();
        }
    }

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-3
    }

    #[test]
    fn regiments_move_at_their_slowest_members_speed() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.order_move(7, 0, [10.0, 20.0]).unwrap();
        run(&mut w, 1.0);
        assert!(near(w.units[0].position, [9.0, 12.0]));
        assert!(near(w.units[1].position, [11.0, 12.0]));
        assert!(w.units.iter().all(Unit::moving));
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [9.0, 20.0]));
        assert!(!w.units.iter().any(Unit::moving));
    }

    #[test]
    fn formations_turn_to_face_the_way_they_go() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        // Facing +x now, so the left-hand man ends up on the +y side.
        w.order_move(7, 0, [30.0, 10.0]).unwrap();
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
        w.order_move(7, 0, [10.0, 20.0]).unwrap();
        run(&mut w, 10.0);
        assert!(!near(w.units[1].position, [11.0, 20.0]));
        w.order_move(7, 0, [10.0, 30.0]).unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [9.0, 30.0]));
        assert!(near(w.units[1].position, [11.0, 30.0]));
    }

    #[test]
    fn changing_formation_rearranges_them_where_they_stand() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.set_formation(7, 0, 1).unwrap();
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
        w.order_move(7, 0, [10.0, 30.0]).unwrap();
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
            }],
            formation: 0,
            combat: None,
            operational_range: None,
            primary: None,
        };
        let r = w.add_regiment(&second);
        assert_eq!((r, w.units[2].regiment), (1, 1));
        w.order_move(7, r, [20.0, 20.0]).unwrap();
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
                    }],
                    formation: 0,
                    combat: None,
                    operational_range: Some(34.0),
                    primary: None,
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
            w.order_move(1, 0, [10.0, 14.0]).unwrap();
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
        });
        r.combat = Some(1);
        w
    }

    #[test]
    fn moving_regiments_stop_to_fight_what_fires_on_them_then_carry_on() {
        let mut w = skirmish();
        w.order_move(1, 0, [10.0, 50.0]).unwrap();
        // In the enemy's range after 4s, at 18.
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert_eq!(w.regiments[0].current_formation(), 1);
        assert!(w.units[0].health < 100.0);
        let stopped = w.units[0].position;
        assert!(stopped[1] < 20.0, "{stopped:?}");
        // Orders to move only take effect once it's done.
        w.order_move(1, 0, [30.0, 20.0]).unwrap();
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
    fn moving_regiments_engage_enemies_they_run_into() {
        let mut w = skirmish();
        // Unarmed, so it doesn't fire on them first.
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.units[0].fighter = Some(soldier(vec![rifle(2.0, 50.0)]));
        w.order_move(1, 0, [10.0, 50.0]).unwrap();
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
        w.order_move(1, 0, [10.0, 50.0]).unwrap();
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
    fn attacking_regiments_engage_what_they_can_fire_on_then_carry_on() {
        let mut w = skirmish();
        // Unarmed, so a regiment only moving would walk on until it ran
        // into it.
        w.units[1].fighter = Some(soldier(Vec::new()));
        w.order_attack(1, 0, [10.0, 50.0]).unwrap();
        // In its range after 4s, at 18.
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(!w.units[0].moving());
        assert!(w.units[0].position[1] < 20.0);
        assert_eq!(w.regiments[0].order, Order::Attack);
        assert!(w.underway(0));
        run(&mut w, 3.0);
        assert!(!w.units[1].alive());
        run(&mut w, 20.0);
        assert!(near(w.units[0].position, [10.0, 50.0]));
        assert_eq!(w.regiments[0].order, Order::Move);
    }

    #[test]
    fn attacking_regiments_leave_friends_alone() {
        let mut w = skirmish();
        w.regiments[1].owner = 1;
        w.order_attack(1, 0, [10.0, 50.0]).unwrap();
        run(&mut w, 6.0);
        assert!(w.regiments[0].engaged.is_empty());
        assert!(w.units[1].alive());
    }

    #[test]
    fn running_regiments_break_off_and_ignore_enemies() {
        let mut w = skirmish();
        w.order_move(1, 0, [10.0, 50.0]).unwrap();
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        w.order_run(1, 0, [10.0, 50.0]).unwrap();
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
        w.order_move(7, 0, [10.0, 20.0]).unwrap();
        run(&mut w, 1.0);
        w.order_stop(7, 0).unwrap();
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
        w.order_move(7, 0, [10.0, 20.0]).unwrap();
        assert!(w.underway(0));
        assert!(near(w.destination(0), [10.0, 20.0]));
        run(&mut w, 30.0);
        assert!(!w.underway(0));
        w.order_move(7, 0, [10.0, 5.0]).unwrap();
        run(&mut w, 0.5);
        w.order_stop(7, 0).unwrap();
        assert!(!near(w.destination(0), [10.0, 5.0]));
    }

    #[test]
    fn fighting_on_the_way_keeps_the_destination() {
        let mut w = skirmish();
        w.order_move(1, 0, [10.0, 50.0]).unwrap();
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        assert!(w.underway(0));
        assert!(near(w.destination(0), [10.0, 50.0]));
    }

    #[test]
    fn stopped_regiments_stay_put_after_fighting() {
        let mut w = skirmish();
        w.order_move(1, 0, [10.0, 50.0]).unwrap();
        run(&mut w, 4.5);
        assert_eq!(w.regiments[0].engaged, [Enemy::Regiment(1)]);
        let held = w.regiments[0].position;
        w.order_stop(1, 0).unwrap();
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
        w.order_move(1, 0, [30.0, 10.0]).unwrap();
        run(&mut w, 15.0);
        assert!(w.buildings.iter().all(|b| b.owner == 1));
        assert!(w.buildings[0].captured.is_some());
        // Leaving doesn't give it back.
        w.order_move(1, 0, [10.0, 10.0]).unwrap();
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
        w.order_move(1, 0, [30.0, 10.0]).unwrap();
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

    #[test]
    fn the_same_orders_play_out_the_same_way() {
        let play = || {
            let rifles = |n| (0..n).map(|_| soldier(vec![rifle(12.0, 25.0)])).collect();
            let mut w = battle(&[1, 2, 3, 2], rifles(4), at_war());
            let house = building(20, 2, [30.0, 10.0]);
            w.pathing.block(house.obstacle.as_ref().unwrap());
            w.add_buildings(&[house]);
            w.order_move(1, 0, [30.0, 30.0]).unwrap();
            w.order_run(3, 2, [10.0, 50.0]).unwrap();
            run(&mut w, 15.0);
            w.order_move(2, 3, [20.0, 20.0]).unwrap();
            run(&mut w, 30.0);
            w
        };
        let (a, b) = (play(), play());
        assert!(a.units.iter().any(|u| u.died.is_some()));
        assert_eq!(snapshot(&a), snapshot(&b));
    }
}
