//! The general's special operations: one goal using each operation the
//! mood lists, as the side's experience allows, at the best place it finds.

use std::collections::HashMap;

use tgmap::sai::{ActorRequirements, GeneralPower, Tally};
use tgmap::{Id, Operation, Relation};

use super::air::fighting;
use super::liking;
use super::picture::Picture;
use super::planner::{Goal, Key, Planner, Work};
use crate::{Balance, Command, Piece, Seen, State, TICK, Target, World};

/// What a ratio of friendly to enemy influence is taken as where there's
/// no enemy influence, per point of friendly.
const NO_ENEMY_RATIO: f32 = 1000.0;

pub(super) fn goals(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let world = picture.world;
    let player = picture.player;
    let Some(budget) = world.experience(player) else {
        return Vec::new();
    };
    // Kept whatever the mood, for the moods that look at it.
    let timed = (planner.moods().iter()).any(|m| {
        m.goal_engine
            .general_powers
            .power
            .iter()
            .any(times_exploring)
    });
    if timed {
        note_explored(planner, picture);
    }
    let powers = planner.ego().goal_engine.general_powers.power.clone();
    if powers.is_empty() {
        return Vec::new();
    }
    keep_order(planner, &powers);
    let mut weighing = Weighing {
        planner,
        picture,
        things: things(world),
        engaged: Default::default(),
        balances: HashMap::new(),
        worths: HashMap::new(),
    };
    let mut left = budget;
    let mut orders = Vec::new();
    let mut priority = 0.0;
    let mut first_found = None;
    let order = weighing.planner.operations.clone();
    for (k, id) in order.iter().enumerate() {
        let Some(power) = powers.iter().find(|p| p.ability_ids.as_ref() == Some(id)) else {
            continue;
        };
        let commander = world.commanders.get(&player);
        let had = commander.and_then(|c| c.operations.iter().position(|op| &op.id == id));
        // One its general doesn't have is never used.
        let Some(i) = had else { continue };
        let cost = commander.expect("had").operations[i].cost;
        if cost > left {
            break;
        }
        left -= cost;
        if let Some((score, at, on)) = weighing.best(power, id) {
            priority += score;
            orders.push(Command::Operate {
                operation: i,
                at,
                on,
            });
            first_found.get_or_insert(k);
        }
    }
    weighing.planner.redraw_from = first_found;
    if priority <= 0.0 {
        return Vec::new();
    }
    vec![Goal {
        key: Key::GeneralPower,
        kind: "GENERAL_POWER",
        priority,
        cost: 0.0,
        work: Work::Orders(orders),
    }]
}

/// Keeps the order the listed operations are weighed in: drawn afresh
/// for a list that's changed, and from the first that found a place last
/// think on, if any did.
fn keep_order(planner: &mut Planner, powers: &[GeneralPower]) {
    let listed: Vec<Id> = powers
        .iter()
        .filter_map(|p| p.ability_ids.clone())
        .collect();
    let mut had = planner.operations.clone();
    let mut wanted = listed.clone();
    had.sort();
    wanted.sort();
    let from = match had == wanted {
        true => match planner.redraw_from.take() {
            Some(from) => from,
            None => return,
        },
        false => {
            planner.operations = listed;
            planner.redraw_from = None;
            0
        }
    };
    let base = |id: &Id| {
        let power = powers.iter().find(|p| p.ability_ids.as_ref() == Some(id));
        power.map_or(0.0, |p| p.priority.base * p.priority.base)
    };
    let mut rest: Vec<Id> = planner
        .operations
        .split_off(from.min(planner.operations.len()));
    while !rest.is_empty() {
        let total: f32 = rest.iter().map(base).sum();
        let i = match total > 0.0 {
            true => {
                let mut pick = planner.chance() * total;
                (0..rest.len())
                    .find(|&i| {
                        pick -= base(&rest[i]);
                        pick < 0.0
                    })
                    .unwrap_or(rest.len() - 1)
            }
            false => ((planner.chance() * rest.len() as f32) as usize).min(rest.len() - 1),
        };
        planner.operations.push(rest.remove(i));
    }
}

/// Whether an operation's places are weighed by how long since their
/// region was explored.
fn times_exploring(power: &GeneralPower) -> bool {
    let t = &power.priority.time_since_explored;
    t.multiplier != 0.0 || t.minimum > 0.0
}

/// Notes the regions its side sees enough of now as explored now.
fn note_explored(planner: &mut Planner, picture: &Picture) {
    let world = picture.world;
    let regions = world.regions.regions.len();
    if planner.explored.len() != regions {
        // One never seen counts from the start of the match.
        planner.explored = vec![0; regions];
    }
    let sharing = world.sharing_sight(picture.player);
    let side = world.pathing.side();
    let (mut seen, mut all) = (vec![0u32; regions], vec![0u32; regions]);
    for y in 0..side as i32 {
        for x in 0..side as i32 {
            let at = crate::Tile { x, y }.centre();
            let Some(region) = world.regions.at(at) else {
                continue;
            };
            all[region] += 1;
            if world.seen_with(&sharing, at) == Seen::Visible {
                seen[region] += 1;
            }
        }
    }
    let share = planner.explored_share();
    for region in 0..regions {
        if all[region] > 0 && seen[region] as f32 >= share * all[region] as f32 {
            planner.explored[region] = world.ticks;
        }
    }
}

/// A unit or a building, which operations look at one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Thing {
    Unit(usize),
    Building(usize),
}

/// One think's weighing of where to use operations.
struct Weighing<'p, 'w> {
    planner: &'p mut Planner,
    picture: &'p Picture<'w>,
    things: Vec<Thing>,
    /// Whether each organization, and building, is fighting, as it's asked.
    engaged: std::cell::RefCell<HashMap<Piece, bool>>,
    /// Each side's economy, as its things' worth is reckoned for it.
    balances: HashMap<u32, Balance>,
    worths: HashMap<(Thing, u32), f32>,
}

impl Weighing<'_, '_> {
    fn world(&self) -> &World {
        self.picture.world
    }

    fn position(&self, t: Thing) -> [f32; 2] {
        let world = self.world();
        match t {
            Thing::Unit(u) => world.units[u].position,
            Thing::Building(b) => world.buildings[b].position,
        }
    }

    fn owner(&self, t: Thing) -> u32 {
        let world = self.world();
        match t {
            Thing::Unit(u) => world.organizations[world.units[u].organization].owner,
            Thing::Building(b) => world.buildings[b].owner,
        }
    }

    fn template(&self, t: Thing) -> &Id {
        let world = self.picture.world;
        match t {
            Thing::Unit(u) => &world.units[u].template,
            Thing::Building(b) => &world.buildings[b].template,
        }
    }

    /// What an order on the thing is given on.
    fn piece(&self, t: Thing) -> Piece {
        match t {
            Thing::Unit(u) => Piece::Organization(self.world().units[u].organization),
            Thing::Building(b) => Piece::Building(b),
        }
    }

    fn health(&self, t: Thing) -> f32 {
        let world = self.world();
        let (have, most) = match t {
            Thing::Unit(u) => (world.units[u].health, world.max_health(u)),
            Thing::Building(b) => (world.buildings[b].health, world.building_max_health(b)),
        };
        if most > 0.0 { have / most } else { 1.0 }
    }

    fn combat_value(&self, t: Thing) -> f32 {
        let world = self.world();
        match t {
            Thing::Unit(u) => world.unit_value(u),
            Thing::Building(b) => world.target_value(Target::Building(b)),
        }
    }

    /// A thing's worth, reckoned for `side`'s economy.
    fn worth(&mut self, t: Thing, side: u32) -> f32 {
        if let Some(&w) = self.worths.get(&(t, side)) {
            return w;
        }
        let world = self.picture.world;
        let balance = (self.balances.entry(side)).or_insert_with(|| world.balance(side));
        let value = match t {
            Thing::Unit(u) => world.unit_value(u),
            Thing::Building(b) => world.target_value(Target::Building(b)),
        };
        let template = match t {
            Thing::Unit(u) => &world.units[u].template,
            Thing::Building(b) => &world.buildings[b].template,
        };
        let w = liking::worth_to(self.planner.ego(), world, balance, template, value);
        self.worths.insert((t, side), w);
        w
    }

    /// Whether a thing passes `req`, judged from the computer player's
    /// side. A test naming no side leaves out only things of no side.
    fn passes(&self, req: &ActorRequirements, t: Thing) -> bool {
        let world = self.world();
        let owner = self.owner(t);
        let relation = world.sides.relation(self.picture.player, owner);
        let friendly = matches!(relation, Relation::Itself | Relation::Ally);
        let sided = req.must_be_friendly || req.must_be_enemy || req.must_be_owned;
        if !sided && world.sides.player(owner).is_none() {
            return false;
        }
        if (req.must_be_friendly && !friendly)
            || (req.must_be_enemy && relation != Relation::Enemy)
            || (req.must_be_owned && owner != self.picture.player)
        {
            return false;
        }
        if self.health(t) > req.max_percent_health + f32::EPSILON {
            return false;
        }
        if req.must_be_engaged || req.must_not_be_engaged {
            let piece = self.piece(t);
            let mut known = self.engaged.borrow_mut();
            let engaged = *known.entry(piece).or_insert_with(|| fighting(world, piece));
            if (req.must_be_engaged && !engaged) || (req.must_not_be_engaged && engaged) {
                return false;
            }
        }
        let template = self.template(t);
        req.required_property.is_empty()
            || (req.required_property.iter())
                .filter_map(|p| p.ids.as_ref())
                .any(|p| liking::has(world, template, p))
    }

    /// Whether its side sees or remembers a thing; its own and its allies'
    /// it always does.
    fn known(&self, t: Thing, seen_units: &[bool]) -> bool {
        let picture = self.picture;
        if !picture.enemy_of(self.owner(t)) {
            return true;
        }
        let piece = self.piece(t);
        let remembered = picture.known.iter().any(|k| k.piece == piece);
        match t {
            Thing::Unit(u) => seen_units[u] || remembered,
            Thing::Building(_) => remembered,
        }
    }

    /// The best place to use operation `id` by `power`, scored: where, and
    /// the thing it's given on, if any.
    fn best(&mut self, power: &GeneralPower, id: &Id) -> Option<(f32, [f32; 2], Option<Piece>)> {
        let world = self.picture.world;
        let location = &power.location;
        let kind = location
            .kind
            .as_ref()
            .map(|k| k.as_str().to_ascii_uppercase());
        let operation = world.rules.operations.get(id);
        let area = match operation {
            Some(Operation::Enchant(e)) => e.area,
            Some(Operation::Strike(s)) => s.area,
            _ => 0.0,
        };
        let mut candidates: Vec<([f32; 2], Option<Thing>)> = Vec::new();
        match kind.as_deref() {
            Some("FOG") => {
                let regions = &world.regions;
                let centres =
                    (regions.main_island().iter()).map(|&g| (regions.regions[g].centre, None));
                candidates.extend(centres);
            }
            Some(near @ ("ON_ACTOR" | "NEAR_ACTOR")) => {
                let req = &location.actor_requirements;
                // Things are tried only from the sides it names.
                if !(req.must_be_friendly || req.must_be_enemy || req.must_be_owned) {
                    return None;
                }
                let seen_units = world.sees_units(self.picture.player);
                let from: Vec<Thing> = (self.things.iter().copied())
                    .filter(|&t| self.passes(&location.actor_requirements, t))
                    .filter(|&t| self.known(t, &seen_units))
                    .collect();
                let size = world.pathing.size();
                for t in from {
                    let here = self.position(t);
                    if near == "ON_ACTOR" {
                        candidates.push((here, Some(t)));
                        continue;
                    }
                    for _ in 0..location.placement.num_candidates {
                        let angle = self.planner.chance() * std::f32::consts::TAU;
                        let off = self.planner.chance() * location.placement.radius;
                        let spot = [
                            here[0] + libm::cosf(angle) * off,
                            here[1] + libm::sinf(angle) * off,
                        ];
                        let spot = match operation {
                            Some(Operation::Summon(summon)) => world.summon_spot(summon, spot),
                            Some(Operation::Drop(template)) => world.drop_spot(template, spot),
                            _ => Some(spot),
                        };
                        let on_map = |at: [f32; 2]| at.iter().all(|c| (0.0..=size).contains(c));
                        if let Some(spot) = spot.filter(|&s| on_map(s)) {
                            candidates.push((spot, None));
                        }
                    }
                }
            }
            _ => return None,
        }
        let fog = kind.as_deref() == Some("FOG");
        let mut best: Option<(f32, [f32; 2], Option<Piece>)> = None;
        for (at, on) in candidates {
            let Some(score) = self.score(power, at, on, area, fog) else {
                continue;
            };
            if best.is_none_or(|(b, _, _)| score > b) {
                best = Some((score, at, on.map(|t| self.piece(t))));
            }
        }
        best.filter(|&(score, _, _)| score > 0.0)
    }

    /// What using `power` at `at`, on the thing `on` for one given on a
    /// thing, scores; none where a factor rules it out.
    fn score(
        &mut self,
        power: &GeneralPower,
        at: [f32; 2],
        on: Option<Thing>,
        area: f32,
        fog: bool,
    ) -> Option<f32> {
        let p = &power.priority;
        let mut score = p.base;
        if !fog {
            score += self.tally(&p.cv, at, on, area, false)?;
            score += self.tally(&p.actor_value, at, on, area, true)?;
        }
        let picture = self.picture;
        let region = picture.region(at);
        let of = |v: &[f32]| region.map_or(0.0, |g| v[g]);
        let enemy = of(&picture.enemy) + of(&picture.enemy_static);
        let e = &p.enemy_influence;
        let v = match enemy > 0.0 {
            true => libm::powf(enemy, e.exponent),
            false => 0.0,
        };
        score += capped(v * e.multiplier, e.maximum, e.minimum)?;
        let r = &p.influence_ratio;
        let friendly = match r.count_owned_only {
            true => of(&picture.owned),
            false => of(&picture.friendly) + of(&picture.friendly_static),
        };
        let ratio = match enemy > 0.0 {
            true => friendly / enemy,
            false => friendly * NO_ENEMY_RATIO,
        };
        let v = match ratio {
            0.0 => 0.0,
            ratio if ratio > 1.0 => libm::powf(ratio, r.exponent_over_one),
            ratio => libm::powf(ratio, r.exponent_under_one),
        };
        score += capped(v * r.multiplier, r.maximum, r.minimum)?;
        let t = &p.time_since_explored;
        let explored = region.and_then(|g| self.planner.explored.get(g));
        let since = explored.map_or(0.0, |&tick| {
            picture.world.ticks.saturating_sub(tick) as f32 * TICK
        });
        score += capped(since * t.multiplier, t.maximum, t.minimum)?;
        let g = &p.economy_generated;
        let mut made = crate::economy::Amounts::default();
        for resource in &g.resource {
            if let Some(id) = &resource.ids {
                made.set(id.as_str(), made.get(id.as_str()) + resource.amount);
            }
        }
        let ego = self.planner.ego();
        let made = liking::made_worth(ego, picture.world, &picture.balance, &made);
        score += capped(made * g.multiplier, -1.0, g.minimum)?;
        if score < p.minimum {
            return None;
        }
        Some(score + self.planner.chance() * p.fuzzy_factor.max(0.0))
    }

    /// What a factor adding up things comes to at `at`: the thing `on`
    /// alone for one given on a thing, else those within `area` of `at`
    /// from the sides it considers. Each counts its combat value, or its
    /// worth, × its multiplier.
    fn tally(
        &mut self,
        tally: &Tally,
        at: [f32; 2],
        on: Option<Thing>,
        area: f32,
        worth: bool,
    ) -> Option<f32> {
        let player = self.picture.player;
        // Nothing counts for anything.
        if tally.multiplier == 0.0 && tally.category.iter().all(|c| c.multiplier == 0.0) {
            return capped(0.0, tally.maximum, tally.minimum);
        }
        let counted: Vec<Thing> = match on {
            Some(t) => vec![t],
            None => {
                let world = self.world();
                let reach = area * area;
                (self.things.iter().copied())
                    .filter(|&t| crate::squared_distance(self.position(t), at) <= reach)
                    .filter(|&t| {
                        let owner = self.owner(t);
                        let relation = world.sides.relation(player, owner);
                        (tally.consider_owned && owner == player)
                            || (tally.consider_friendly
                                && matches!(relation, Relation::Itself | Relation::Ally))
                            || (tally.consider_enemy && relation == Relation::Enemy)
                    })
                    .collect()
            }
        };
        let mut sum = 0.0;
        for t in counted {
            // The last category it matches wins.
            let category = (tally.category.iter()).rev().find(|c| self.passes(c, t));
            let multiplier = category.map_or(tally.multiplier, |c| c.multiplier);
            if multiplier == 0.0 {
                continue;
            }
            let value = match worth {
                // A target's worth is reckoned for the computer player,
                // anything else's for its own side.
                true => {
                    let side = if on.is_some() { player } else { self.owner(t) };
                    self.worth(t, side)
                }
                false => self.combat_value(t),
            };
            sum += value * multiplier;
        }
        capped(sum, tally.maximum, tally.minimum)
    }
}

/// `v` held to `maximum` when that's 0 or more; none when it's below
/// `minimum`.
fn capped(v: f32, maximum: f32, minimum: f32) -> Option<f32> {
    let v = match maximum >= 0.0 {
        true => v.min(maximum),
        false => v,
    };
    (v >= minimum).then_some(v)
}

/// Every thing on the map that counts: living units, and buildings
/// standing.
fn things(world: &World) -> Vec<Thing> {
    let units = (0..world.units.len())
        .filter(|&u| world.units[u].alive())
        .map(Thing::Unit);
    let buildings = (0..world.buildings.len())
        .filter(|&b| world.buildings[b].state == State::Standing)
        .map(Thing::Building);
    units.chain(buildings).collect()
}

#[cfg(test)]
mod tests {
    use super::capped;

    #[test]
    fn a_factor_is_capped_before_its_minimum_is_tested() {
        assert_eq!(capped(150.0, 100.0, 120.0), None);
        assert_eq!(capped(150.0, 100.0, 0.0), Some(100.0));
        // A negative maximum is none.
        assert_eq!(capped(150.0, -1.0, 0.0), Some(150.0));
        assert_eq!(capped(50.0, -1.0, 60.0), None);
    }
}
