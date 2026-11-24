//! How one planning think shares its units out among its goals, and
//! which goals it carries out.

use std::collections::HashMap;

use super::liking;
use super::picture::Picture;
use super::planner::{Goal, Key, Planner, Work};
use crate::Command;

/// How near a unit already sent somewhere has to be headed to the place a
/// goal wants it for the order not to be given again.
const STILL_HEADED: f32 = 10.0;

/// How many times one think settles a goal: once, and once more if units
/// taken from it drop it below goals not yet settled.
const MOST_VISITS: u8 = 2;

/// What the goals of one think come to.
pub(super) struct Chosen {
    pub orders: Vec<Command>,
    /// The goal each unit serves.
    pub serving: HashMap<usize, Key>,
    /// The goals carried out.
    pub carried: Vec<Key>,
}

/// Shares the units out among `goals`, highest first, and carries out
/// those that come out worth anything, paying for them in turn.
pub(super) fn choose(planner: &Planner, picture: &Picture, goals: Vec<Goal>) -> Chosen {
    let mut board = Board::new(planner, picture, goals);
    board.settle_all();
    board.carry_out()
}

/// The goals of one think and the units each holds as they're settled.
struct Board<'p, 'w> {
    planner: &'p Planner,
    picture: &'p Picture<'w>,
    goals: Vec<Goal>,
    /// Where each goal stands in the order: its priority until it's
    /// settled, then its worth with the units it has.
    rank: Vec<f32>,
    members: Vec<Vec<usize>>,
    /// The goal holding each unit.
    holder: HashMap<usize, usize>,
    /// Each goal's stored worth: with the units it held at the start of
    /// the think, then with those it ends up with once it's settled.
    stored: Vec<f32>,
    /// For each fight, the steps from its region to every other.
    steps: Vec<Option<Vec<Option<u32>>>>,
    /// For each fight, those of its kind near enough to share its units'
    /// strength.
    linked: Vec<Vec<usize>>,
    /// Each unit's place in `units`.
    slot: HashMap<usize, usize>,
    units: Vec<Facts>,
    /// For each fight and explore goal, how each unit in `units` fits it.
    fits: Vec<Vec<Fit>>,
    visits: Vec<u8>,
    /// Whether each goal is settled for now.
    settled: Vec<bool>,
    paid: Vec<bool>,
    /// The money left to pay for goals with.
    left: f32,
    /// What this think has queued at each building.
    queued: HashMap<usize, usize>,
}

/// What a think needs to know of one of its units, worked out once.
struct Facts {
    /// What its strength counts for in a fight.
    strength: f32,
    builder: bool,
    region: Option<usize>,
}

/// How a unit fits a fight or explore goal.
#[derive(Clone, Copy, Default)]
struct Fit {
    serves: bool,
    /// Seconds it takes to get there.
    seconds: f32,
    /// What the goal's preferences add for it, and multiply by.
    value: f32,
    multiplier: f32,
}

impl<'p, 'w> Board<'p, 'w> {
    fn new(planner: &'p Planner, picture: &'p Picture<'w>, goals: Vec<Goal>) -> Self {
        let n = goals.len();
        let world = picture.world;
        let combat = &planner.ego().goal_engine.combat;
        let steps: Vec<Option<Vec<Option<u32>>>> = (goals.iter())
            .map(|goal| match goal.work {
                Work::Force { region, .. } => Some(world.regions.steps(region)),
                _ => None,
            })
            .collect();
        let linked = (0..n)
            .map(|g| {
                let (Work::Force { attack, .. }, Some(steps)) = (&goals[g].work, &steps[g]) else {
                    return Vec::new();
                };
                let near = |h: usize| match goals[h].work {
                    Work::Force {
                        region, attack: a, ..
                    } => {
                        a == *attack
                            && steps[region].is_some_and(|s| s <= combat.linked_region_distance)
                    }
                    _ => false,
                };
                (0..n).filter(|&h| h != g && near(h)).collect()
            })
            .collect();
        let ego = planner.ego();
        let units: Vec<Facts> = (picture.organizations.iter())
            .map(|&r| {
                let builder = !world.buildable(r).is_empty();
                // Builders count for little in a fight.
                let share = match builder {
                    true => combat.penalty_to_builder_cv,
                    false => 1.0,
                };
                Facts {
                    strength: world.full_value(r) * share,
                    builder,
                    region: picture.region(world.centre(r)),
                }
            })
            .collect();
        let slot = (picture.organizations.iter().enumerate())
            .map(|(i, &r)| (r, i))
            .collect();
        let fits = (goals.iter())
            .map(|goal| {
                let (region, explore) = match &goal.work {
                    Work::Force { region, .. } => (*region, false),
                    Work::Explore { to } => match picture.region(*to) {
                        Some(region) => (region, true),
                        None => return vec![Fit::default(); units.len()],
                    },
                    _ => return Vec::new(),
                };
                (picture.organizations.iter())
                    .map(|&r| {
                        let seconds = liking::travel_time(picture, r, region);
                        let serves = match explore {
                            true => world.explores(r) && world.organizations[r].unpacks.is_none(),
                            false => {
                                picture.fights(r)
                                    && !planner.retreating.contains(&r)
                                    && seconds.is_finite()
                            }
                        };
                        let (value, multiplier) = liking::preference(ego, world, goal.kind, r);
                        Fit {
                            serves,
                            seconds,
                            value,
                            multiplier,
                        }
                    })
                    .collect()
            })
            .collect();
        let mut board = Board {
            planner,
            picture,
            rank: goals.iter().map(|g| g.priority).collect(),
            members: vec![Vec::new(); n],
            holder: HashMap::new(),
            stored: vec![0.0; n],
            steps,
            linked,
            slot,
            units,
            fits,
            visits: vec![0; n],
            settled: vec![false; n],
            paid: vec![false; n],
            left: world.stockpile(picture.player),
            queued: HashMap::new(),
            goals,
        };
        // Each goal starts the think with the units it held at the end of
        // the last.
        let index: HashMap<Key, usize> = (board.goals.iter().enumerate())
            .map(|(g, goal)| (goal.key.clone(), g))
            .collect();
        for &r in &picture.organizations {
            let held = planner.serving.get(&r).and_then(|k| index.get(k));
            if let Some(&g) = held
                && board.serves(g, r)
            {
                board.take(g, r);
            }
        }
        for g in 0..n {
            board.stored[g] = board.worth(g, &board.members[g]);
        }
        board
    }

    /// Whether the unit at `r` could serve goal `g` at all.
    fn serves(&self, g: usize, r: usize) -> bool {
        match &self.goals[g].work {
            Work::Order(_) => false,
            Work::Unit { organization, .. } => *organization == r,
            Work::Force { .. } | Work::Explore { .. } => self.fit(g, r).serves,
        }
    }

    fn fit(&self, g: usize, r: usize) -> Fit {
        self.fits[g][self.slot[&r]]
    }

    fn strength(&self, r: usize) -> f32 {
        self.units[self.slot[&r]].strength
    }

    /// Goal `g`'s worth with the units `set` on it.
    fn worth(&self, g: usize, set: &[usize]) -> f32 {
        let goal = &self.goals[g];
        let ego = self.planner.ego();
        match &goal.work {
            Work::Order(_) => goal.priority,
            Work::Unit { organization, .. } => match set.contains(organization) {
                true => goal.priority,
                false => 0.0,
            },
            Work::Explore { .. } => (set.iter())
                .map(|&r| {
                    let fit = self.fit(g, r);
                    let travel = liking::travel_factor(ego, fit.seconds);
                    (goal.priority + fit.value) * fit.multiplier * travel
                })
                .fold(0.0, f32::max),
            Work::Force { strength, .. } => {
                if set.is_empty() {
                    return 0.0;
                }
                let (mut on, mut value, mut multiplier, mut seconds) = (0.0, 0.0, 1.0, 0.0);
                for &r in set {
                    let fit = self.fit(g, r);
                    on += self.strength(r);
                    value += fit.value;
                    multiplier *= fit.multiplier;
                    seconds += fit.seconds;
                }
                let travel = liking::travel_factor(ego, seconds / set.len() as f32);
                let linked = self.linked_strength(g, set);
                (goal.priority * strength.factor(on + linked) + value) * multiplier * travel
            }
        }
    }

    /// The strength of the units on the fights linked to `g`, leaving out
    /// those in `set`.
    fn linked_strength(&self, g: usize, set: &[usize]) -> f32 {
        (self.linked[g].iter())
            .flat_map(|&h| &self.members[h])
            .filter(|r| !set.contains(r))
            .map(|&r| self.strength(r))
            .sum()
    }

    /// How far goal `g`, and the fights linked to it, which share its
    /// units' strength, are worth more than their stored worth.
    fn above_stored(&self, g: usize) -> f32 {
        (std::iter::once(g).chain(self.linked[g].iter().copied()))
            .map(|k| self.worth(k, &self.members[k]) - self.stored[k])
            .sum()
    }

    /// What moving the unit at `r` to goal `g` gains it over its stored
    /// worth, across the fights linked to it, among which a fight it's
    /// taken from counts its loss.
    fn gain(&mut self, g: usize, r: usize) -> f32 {
        let from = self.holder.get(&r).copied().filter(|&h| h != g);
        let had = from.map(|h| self.members[h].clone());
        if let Some(h) = from {
            self.members[h].retain(|&m| m != r);
        }
        self.members[g].push(r);
        let gain = self.above_stored(g);
        self.members[g].pop();
        if let (Some(h), Some(had)) = (from, had) {
            self.members[h] = had;
        }
        gain
    }

    /// What taking the unit at `r` from goal `h` loses it from its stored
    /// worth, across the fights linked to it.
    fn loss(&mut self, h: usize, r: usize) -> f32 {
        let had = self.members[h].clone();
        self.members[h].retain(|&m| m != r);
        let loss = -self.above_stored(h);
        self.members[h] = had;
        loss
    }

    /// Whether the unit at `r` is held to goal `h`: it's an attack, and the
    /// unit is near enough to have locked on to it.
    fn locked(&self, h: usize, r: usize) -> bool {
        let Work::Force { attack: true, .. } = self.goals[h].work else {
            return false;
        };
        let unit = &self.units[self.slot[&r]];
        if unit.builder {
            return false;
        }
        let commit = self.planner.ego().goal_engine.combat.attack_commit_distance;
        let steps = self.steps[h].as_ref();
        (unit.region.zip(steps)).is_some_and(|(at, steps)| steps[at].is_some_and(|s| s <= commit))
    }

    /// Whether goal `g` wants more units than it has.
    fn wants_more(&self, g: usize) -> bool {
        match &self.goals[g].work {
            Work::Order(_) => false,
            Work::Unit { .. } | Work::Explore { .. } => self.members[g].is_empty(),
            Work::Force { strength, .. } => {
                let on: f32 = self.members[g].iter().map(|&r| self.strength(r)).sum();
                on + self.linked_strength(g, &[]) + strength.given < strength.target
            }
        }
    }

    /// Moves the unit at `r` to goal `g`, from the goal holding it, which
    /// is met again if it was settled.
    fn take(&mut self, g: usize, r: usize) {
        let Some(h) = self.holder.insert(r, g) else {
            self.members[g].push(r);
            return;
        };
        self.members[h].retain(|&m| m != r);
        self.members[g].push(r);
        if self.settled[h] {
            self.rank[h] = self.worth(h, &self.members[h]);
            if self.rank[h] <= 0.0 {
                self.refund(h);
            }
            if self.visits[h] < MOST_VISITS {
                self.settled[h] = false;
            }
        }
    }

    /// Gives back what goal `g` was paid, once it's no longer carried out.
    fn refund(&mut self, g: usize) {
        if !std::mem::take(&mut self.paid[g]) {
            return;
        }
        self.left += self.goals[g].cost;
        if let Some(b) = queue_at(&self.goals[g]) {
            *self.queued.entry(b).or_default() -= 1;
        }
    }

    fn release(&mut self, g: usize) {
        for r in std::mem::take(&mut self.members[g]) {
            self.holder.remove(&r);
        }
    }

    /// Fills goal `g` with units: free ones first, then those of goals
    /// ranked below it from the lowest up, then those of goals ranked above
    /// it from the nearest up while it gains more than they lose.
    fn fill(&mut self, g: usize) {
        if let Work::Unit { organization, .. } = self.goals[g].work {
            // It takes its one unit from wherever it is, when it's worth
            // more here than there.
            let r = organization;
            let take = match self.holder.get(&r) {
                None => true,
                Some(&h) => h != g && self.goals[g].priority > self.loss(h, r),
            };
            if take {
                self.take(g, r);
            }
            return;
        }
        if !self.wants_more(g) {
            return;
        }
        let free: Vec<usize> = (self.picture.organizations.iter().copied())
            .filter(|r| !self.holder.contains_key(r) && self.serves(g, *r))
            .collect();
        self.take_any(g, free);
        let others = |board: &Self, above: bool| {
            let mut out: Vec<usize> = (0..board.goals.len())
                .filter(|&h| h != g && (board.rank[h] >= board.rank[g]) == above)
                .collect();
            out.sort_by(|&a, &b| board.rank[a].total_cmp(&board.rank[b]));
            out
        };
        for h in others(self, false) {
            if !self.wants_more(g) {
                return;
            }
            let theirs: Vec<usize> = (self.members[h].iter().copied())
                .filter(|&r| self.serves(g, r) && !self.locked(h, r))
                .collect();
            self.take_any(g, theirs);
        }
        for h in others(self, true) {
            // A linked fight's loss is already counted in the gain.
            let linked = self.linked[g].contains(&h);
            for r in self.members[h].clone() {
                if !self.wants_more(g) {
                    return;
                }
                if !self.serves(g, r) || self.locked(h, r) {
                    continue;
                }
                let loss = match linked {
                    true => 0.0,
                    false => self.loss(h, r),
                };
                if self.gain(g, r) <= loss {
                    break;
                }
                self.take(g, r);
            }
        }
    }

    /// Takes units from `candidates` that gain goal `g` anything, while it
    /// wants more.
    fn take_any(&mut self, g: usize, candidates: Vec<usize>) {
        for r in candidates {
            if !self.wants_more(g) {
                return;
            }
            if self.gain(g, r) > 0.0 {
                self.take(g, r);
            }
        }
    }

    /// Settles each goal in turn, highest-ranked first, paying for those
    /// worth carrying out.
    fn settle_all(&mut self) {
        let world = self.picture.world;
        let engine = &self.planner.ego().goal_engine;
        let saving_for = (self.picture.money_rate() * engine.resource_management.saving_time_limit
            / 60.0)
            .max(0.0);
        let mut saving = false;
        let most_queued = engine.build_queue_max_items.max(1) as usize;
        loop {
            let next = (0..self.goals.len())
                .filter(|&g| !self.settled[g])
                .max_by(|&a, &b| self.rank[a].total_cmp(&self.rank[b]));
            let Some(g) = next else { break };
            self.visits[g] += 1;
            self.settled[g] = true;
            self.fill(g);
            let mut worth = self.worth(g, &self.members[g]);
            let goal = &self.goals[g];
            if worth > 0.0 && !self.paid[g] {
                let queue = queue_at(goal);
                let full = queue.is_some_and(|b| {
                    let had = world.buildings[b].queue.len();
                    had + self.queued.get(&b).copied().unwrap_or(0) >= most_queued
                });
                // An order the match would refuse isn't carried out, nor
                // paid for.
                let refused = match &goal.work {
                    Work::Order(command) => world.check(self.picture.player, command).is_err(),
                    _ => false,
                };
                if full || refused {
                    worth = 0.0;
                } else if goal.cost > 0.0 {
                    if saving || goal.cost > self.left + saving_for {
                        worth = 0.0;
                    } else if goal.cost > self.left {
                        // Saves for it, paying for nothing below it.
                        saving = true;
                        worth = 0.0;
                    }
                }
                if worth > 0.0 {
                    self.left -= goal.cost;
                    self.paid[g] = true;
                    if let Some(b) = queue {
                        *self.queued.entry(b).or_default() += 1;
                    }
                }
            }
            self.stored[g] = worth;
            if worth <= 0.0 {
                self.release(g);
                self.refund(g);
            }
            self.rank[g] = worth;
        }
    }

    /// The orders of the goals carried out, highest-ranked first.
    fn carry_out(self) -> Chosen {
        let picture = self.picture;
        let world = picture.world;
        let mut order: Vec<usize> = (0..self.goals.len())
            .filter(|&g| self.rank[g] > 0.0)
            .collect();
        order.sort_by(|&a, &b| self.rank[b].total_cmp(&self.rank[a]));
        let mut orders = Vec::new();
        let mut serving = HashMap::new();
        let mut carried = Vec::new();
        for g in order {
            let goal = &self.goals[g];
            let mut given: Vec<Command> = Vec::new();
            for &r in &self.members[g] {
                serving.insert(r, goal.key.clone());
            }
            match &goal.work {
                Work::Order(command) => given.push(command.clone()),
                Work::Unit { order, .. } => given.extend(order.iter().cloned()),
                Work::Force { to, attack, .. } => {
                    for &r in &self.members[g] {
                        let again = self.planner.serving.get(&r) == Some(&goal.key)
                            && crate::distance(world.destination(r), *to) <= STILL_HEADED;
                        if again {
                            continue;
                        }
                        given.push(match attack {
                            true => Command::Attack {
                                organization: r,
                                to: *to,
                            },
                            false => Command::Move {
                                organization: r,
                                to: *to,
                            },
                        });
                    }
                }
                Work::Explore { to } => {
                    for &r in &self.members[g] {
                        let again = self.planner.serving.get(&r) == Some(&goal.key)
                            && crate::distance(world.destination(r), *to) <= STILL_HEADED * 4.0;
                        if !again {
                            given.push(Command::Explore {
                                organization: r,
                                to: *to,
                            });
                        }
                    }
                }
            }
            // Orders the match would refuse, such as to a unit routing, are
            // left out.
            given.retain(|c| world.check(picture.player, c).is_ok());
            log::debug!(
                "{}: {} {:?} at {:.0}, costing {:.0}, units {:?}, {} orders",
                picture.player,
                goal.kind,
                goal.key,
                self.rank[g],
                goal.cost,
                self.members[g],
                given.len()
            );
            carried.push(goal.key.clone());
            orders.extend(given);
        }
        Chosen {
            orders,
            serving,
            carried,
        }
    }
}

/// The building a goal queues something at.
fn queue_at(goal: &Goal) -> Option<usize> {
    match &goal.work {
        Work::Order(Command::Commission { building, .. } | Command::Research { building, .. }) => {
            Some(*building)
        }
        _ => None,
    }
}
