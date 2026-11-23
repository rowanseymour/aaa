//! A computer player that plays by a profile: it keeps goals, weighs
//! them each time it plans, and carries out those that come out best.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tgmap::Id;
use tgmap::sai::{Ego, Profile};

use super::picture::{Known, Picture};
use super::{ComputerPlayer, economy, fighting};
use crate::{Command, SECOND, World};

/// What a goal is about, which it keeps from think to think.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum Key {
    Recruit(Id),
    Research(Id),
    Unpack(usize),
    Attach(usize),
    Construct(usize),
    Attack(usize),
    Defend(usize),
    Explore(u32),
    Recover(usize),
}

/// A goal as one planning think weighs it.
#[derive(Debug, Clone)]
pub(super) struct Goal {
    pub key: Key,
    /// The goal id its mood's weights go by, such as `RECRUIT`.
    pub kind: &'static str,
    pub priority: f32,
    /// The money it takes.
    pub cost: f32,
    pub work: Work,
}

/// What carrying a goal out takes.
#[derive(Debug, Clone)]
pub(super) enum Work {
    /// An order for no unit of its own, such as to recruit.
    Order(Command),
    /// One particular organization, given this order, or held where it is.
    Unit {
        organization: usize,
        order: Option<Command>,
    },
    /// Fighting units gathered at `to` in `region`, as many as `strength`
    /// asks for.
    Force {
        region: usize,
        to: [f32; 2],
        attack: bool,
        strength: Wanted,
    },
    /// The organization that likes exploring best, sent to explore at
    /// `to`.
    Explore { to: [f32; 2] },
}

/// How much strength a fight wants, and how much its worth grows with it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Wanted {
    pub target: f32,
    pub enough: f32,
    pub exponent: f32,
    pub over: f32,
    /// Strength it has there without units of its own on the goal.
    pub given: f32,
}

impl Wanted {
    /// What a goal's priority is multiplied by with strength `c` on it.
    pub fn factor(&self, c: f32) -> f32 {
        let c = c + self.given;
        if c < self.target {
            libm::powf(c / self.target, self.exponent)
        } else if c >= self.enough || self.enough <= self.target {
            self.over
        } else {
            1.0 + (self.over - 1.0) * (c - self.target) / (self.enough - self.target)
        }
    }
}

/// A computer player that plays by a profile from the data.
pub struct Planner {
    profile: Arc<Profile>,
    /// Its current mood, by index into the profile's.
    mood: usize,
    rng: u64,
    /// The tick of its next plan and reaction.
    next_plan: u64,
    next_react: u64,
    /// The share of a period early its second plan comes.
    stagger: f32,
    planned: u32,
    /// The random amount each goal carried out last think had.
    carried: HashMap<Key, f32>,
    /// The random amount each goal weighed this think has.
    pending: HashMap<Key, f32>,
    /// The goal each organization served last think.
    pub(super) serving: HashMap<usize, Key>,
    /// The enemy things it saw.
    memory: Vec<Known>,
    /// The region each truck was last sent to unpack in.
    pub(super) unpacking: HashMap<usize, usize>,
    /// Where each organization it's sent to recover is headed.
    pub(super) recovering_at: HashMap<usize, [f32; 2]>,
    /// The building each region's construct goal settled on, and where.
    pub(super) sites: HashMap<usize, (Id, [f32; 2])>,
    /// The region each explore goal last chose.
    pub(super) exploring: HashMap<u32, usize>,
    /// The organizations it's pulled back from a losing fight.
    pub(super) retreating: HashSet<usize>,
    warned: bool,
}

impl Planner {
    /// A computer player playing by `profile`, drawing its chances from
    /// `seed`, its plans after the first coming `stagger` of a period
    /// early so that several don't plan together.
    pub fn new(profile: Arc<Profile>, seed: u64, stagger: f32) -> Planner {
        Planner {
            profile,
            mood: 0,
            rng: seed | 1,
            next_plan: 0,
            next_react: 0,
            stagger: stagger.clamp(0.0, 1.0),
            planned: 0,
            carried: HashMap::new(),
            pending: HashMap::new(),
            serving: HashMap::new(),
            memory: Vec::new(),
            unpacking: HashMap::new(),
            recovering_at: HashMap::new(),
            sites: HashMap::new(),
            exploring: HashMap::new(),
            retreating: HashSet::new(),
            warned: false,
        }
    }

    /// The mood it's in.
    pub(super) fn ego(&self) -> &Ego {
        &self.profile.moods[self.mood]
    }

    /// The id of the mood it's in.
    pub fn mood(&self) -> Option<&Id> {
        self.profile.moods.get(self.mood)?.ids.as_ref()
    }

    /// A number from 0 up to 1.
    pub(super) fn chance(&mut self) -> f32 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A random amount from 0 up to `most`.
    pub(super) fn fuzz(&mut self, most: f32) -> f32 {
        self.chance() * most.max(0.0)
    }

    /// Keeps its mood while the mood's filters hold, else picks another
    /// whose filters do.
    fn choose_mood(&mut self, picture: &Picture) {
        let rate = picture.money_rate();
        let fits = |ego: &Ego| {
            ego.filters.item.iter().all(|f| {
                let value = match f.stat.as_ref().map(Id::as_str) {
                    Some(s) if s.eq_ignore_ascii_case("money_rate") => rate,
                    // A statistic it doesn't keep doesn't rule a mood out.
                    _ => return true,
                };
                f.min <= value && value <= f.max
            })
        };
        if fits(self.ego()) {
            return;
        }
        let profile = self.profile.clone();
        let mut best: Option<(usize, f32)> = None;
        for (i, ego) in profile.moods.iter().enumerate() {
            if !fits(ego) {
                continue;
            }
            let score = ego.priority + self.fuzz(profile.ego_fuzziness);
            if best.is_none_or(|(_, b)| score > b) {
                best = Some((i, score));
            }
        }
        match best {
            Some((i, _)) => self.mood = i,
            None => {
                self.mood = 0;
                if !self.warned {
                    self.warned = true;
                    log::warn!("{}: no mood fits the match; keeping the first", profile.id);
                }
            }
        }
    }

    /// One planning think's orders.
    fn plan(&mut self, world: &World, player: u32) -> Vec<Command> {
        let mut memory = std::mem::take(&mut self.memory);
        let picture = Picture::new(world, player, &mut memory);
        self.memory = memory;
        self.choose_mood(&picture);
        self.retreating
            .retain(|&r| picture.organizations.contains(&r));
        log::debug!(
            "{player}: plans in {:?}, with {:.0} and {:.0} a minute",
            self.mood(),
            world.stockpile(player),
            picture.money_rate()
        );
        let mut goals = Vec::new();
        goals.extend(economy::goals(self, &picture));
        goals.extend(fighting::goals(self, &picture));
        self.weigh(&mut goals);
        let mut orders = self.choose(&picture, goals);
        orders.truncate(self.profile.max_ge_commands);
        orders
    }

    /// Changes each goal's priority by its mood's weights and adds its
    /// random amount, dropping the idle, highest first.
    fn weigh(&mut self, goals: &mut Vec<Goal>) {
        let settings = self.ego().goal_engine.general_settings.clone();
        let engine = self.ego().goal_engine.clone();
        goals.retain(|g| g.priority > 0.0);
        for goal in goals.iter_mut() {
            let weights = engine.weights(goal.kind);
            let (mut fuzziness, mut inertia) = (settings.fuzziness, settings.goal_inertia);
            if let Some(w) = weights {
                goal.priority = goal.priority * w.multiplier + w.bonus;
                if goal.priority < w.minimum {
                    goal.priority = 0.0;
                }
                if w.fuzziness >= 0.0 {
                    fuzziness = w.fuzziness;
                }
                if w.inertia >= 0.0 {
                    inertia = w.inertia;
                }
            }
            if goal.priority <= 0.0 {
                continue;
            }
            // A goal carried out last think keeps its random amount, with
            // the inertia on top.
            let (random, kept) = match self.carried.get(&goal.key) {
                Some(&kept) => (kept, inertia),
                None => (self.fuzz(fuzziness), 0.0),
            };
            goal.priority += random + kept;
            self.pending.insert(goal.key.clone(), random);
        }
        goals.retain(|g| g.priority > 0.0);
        goals.sort_by(|a, b| b.priority.total_cmp(&a.priority));
    }

    /// Carries out the goals that come out best, highest first, paying for
    /// them in turn: the orders they give.
    fn choose(&mut self, picture: &Picture, goals: Vec<Goal>) -> Vec<Command> {
        let engine = self.ego().goal_engine.clone();
        let mut left = picture.world.stockpile(picture.player);
        let saving_for = picture.money_rate() * engine.resource_management.saving_time_limit / 60.0;
        let mut saving = false;
        let mut taken: HashSet<usize> = HashSet::new();
        let mut serving = HashMap::new();
        let mut carried = HashMap::new();
        let mut orders = Vec::new();
        // What this think has queued at each building.
        let mut queued: HashMap<usize, usize> = HashMap::new();
        let most_queued = engine.build_queue_max_items.max(1) as usize;
        for goal in goals {
            let queue_at = match &goal.work {
                Work::Order(
                    Command::Commission { building, .. } | Command::Research { building, .. },
                ) => Some(*building),
                _ => None,
            };
            if let Some(b) = queue_at {
                let had = picture.world.buildings[b].queue.len();
                if had + queued.get(&b).copied().unwrap_or(0) >= most_queued {
                    continue;
                }
            }
            // One whose unit is taken can't go ahead, so isn't saved for.
            if let Work::Unit { organization, .. } = &goal.work
                && taken.contains(organization)
            {
                continue;
            }
            if goal.cost > 0.0 {
                if saving || goal.cost > left + saving_for.max(0.0) {
                    continue;
                }
                if goal.cost > left {
                    // Saves for it, paying for nothing below it.
                    saving = true;
                    continue;
                }
            }
            let given = match &goal.work {
                Work::Order(command) => Some(vec![command.clone()]),
                Work::Unit {
                    organization,
                    order,
                } => {
                    if taken.contains(organization) {
                        None
                    } else {
                        taken.insert(*organization);
                        serving.insert(*organization, goal.key.clone());
                        Some(order.iter().cloned().collect())
                    }
                }
                Work::Force {
                    region,
                    to,
                    attack,
                    strength,
                } => fighting::gather(
                    self,
                    picture,
                    &goal,
                    (*region, *to, *attack, *strength),
                    &mut taken,
                    &mut serving,
                ),
                Work::Explore { to } => {
                    fighting::send_explorer(self, picture, &goal, *to, &mut taken, &mut serving)
                }
            };
            let Some(mut given) = given else {
                continue;
            };
            // Orders the match would refuse, such as to a unit routing, are
            // left out.
            given.retain(|c| picture.world.check(picture.player, c).is_ok());
            // An order refused after all isn't carried out, nor paid for.
            if matches!(goal.work, Work::Order(_)) && given.is_empty() {
                continue;
            }
            log::debug!(
                "{}: {} {:?} at {:.0}, costing {:.0}, {} orders",
                picture.player,
                goal.kind,
                goal.key,
                goal.priority,
                goal.cost,
                given.len()
            );
            left -= goal.cost;
            if let Some(b) = queue_at {
                *queued.entry(b).or_default() += 1;
            }
            if let Some(&fuzz) = self.pending.get(&goal.key) {
                carried.insert(goal.key.clone(), fuzz);
            }
            orders.extend(given);
        }
        self.carried = carried;
        self.pending.clear();
        self.serving = serving;
        orders
    }

    /// The period between plans, in ticks.
    fn plan_period(&self) -> u64 {
        let seconds = self.ego().goal_engine.general_settings.think_frequency;
        ((seconds.max(0.1) * SECOND as f32) as u64).max(1)
    }

    fn react_period(&self) -> u64 {
        let seconds = self.ego().reactive_ai.think_frequency;
        ((seconds.max(0.1) * SECOND as f32) as u64).max(1)
    }
}

impl ComputerPlayer for Planner {
    fn think(&mut self, world: &World, player: u32) -> Vec<Command> {
        // Switched off, it forgets nothing, and carries on once it's back.
        let off = !world.computer_may_play(player);
        if off || self.profile.moods.is_empty() || world.regions.regions.is_empty() {
            return Vec::new();
        }
        let mut orders = Vec::new();
        if world.ticks >= self.next_plan {
            orders.extend(self.plan(world, player));
            let period = self.plan_period();
            self.next_plan = world.ticks + period;
            if self.planned == 0 {
                self.next_plan -= (period as f32 * self.stagger) as u64;
            }
            self.planned += 1;
        }
        if world.ticks >= self.next_react {
            let mut reacted = fighting::react(self, world, player);
            reacted.truncate(self.profile.max_rai_commands);
            orders.extend(reacted);
            self.next_react = world.ticks + self.react_period();
        }
        orders
    }
}
