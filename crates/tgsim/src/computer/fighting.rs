//! The goals that fight: attacking and defending regions, recovering
//! hurt units and exploring; and the reactions between plans.

use std::collections::BTreeMap;

use tgmap::sai::Ego;

use super::liking;
use super::picture::Picture;
use super::planner::{Goal, Key, Member, Planner, Wanted, Work};
use crate::{Command, Order, Piece, Seen, Target, World};

/// How near a unit already sent somewhere has to be headed to the place a
/// goal wants it for the order not to be given again.
const STILL_HEADED: f32 = 10.0;

/// How many regions at random each explore goal weighs.
const EXPLORE_CANDIDATES: usize = 12;

pub(super) fn goals(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let mut out = attack(planner, picture);
    out.extend(defend(planner, picture));
    out.extend(recover(planner, picture));
    out.extend(explore(planner, picture));
    out
}

/// What a known enemy thing is worth attacking.
pub(super) fn target_worth(ego: &Ego, picture: &Picture, piece: Piece) -> f32 {
    let world = picture.world;
    let value = match piece {
        Piece::Organization(r) => world.full_value(r),
        Piece::Building(b) => world.target_value(Target::Building(b)),
    };
    let templates = picture.templates(piece);
    liking::worth(ego, picture, &templates, picture.unpacks(piece), value)
        + liking::priority(ego, picture, &templates, true)
}

/// How much strength a fight in `region` wants, by the mood's curve for
/// attacking or defending.
fn wanted(ego: &Ego, picture: &Picture, region: usize, attack: bool) -> Wanted {
    let engine = &ego.goal_engine;
    let curve = if attack {
        &engine.offensive_cv
    } else {
        &engine.defensive_cv
    };
    let enemy = picture.enemy[region] + picture.enemy_static[region];
    let target = curve
        .min_cv_to_assign
        .max(enemy * curve.cv_advantage_target);
    let given = match !attack && curve.count_static_cv {
        true => picture.friendly_static[region],
        false => 0.0,
    };
    Wanted {
        target,
        enough: target * curve.cv_advantage_max,
        exponent: curve.cv_exponent_under_target,
        over: curve.cv_max_mult_over_target,
        given,
    }
}

/// A goal for each region holding enemy things it knows of, aimed at the
/// one worth most there.
fn attack(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let ego = planner.ego();
    let mut best: BTreeMap<usize, (f32, [f32; 2])> = BTreeMap::new();
    for known in &picture.known {
        let Some(region) = picture.region(known.at) else {
            continue;
        };
        let worth = target_worth(ego, picture, known.piece);
        // One remembered, not seen, is looked for at the region's centre.
        let at = match known.seen {
            true => known.at,
            false => picture.world.regions.regions[region].centre,
        };
        if best.get(&region).is_none_or(|(w, _)| worth > *w) {
            best.insert(region, (worth, at));
        }
    }
    (best.into_iter())
        .map(|(region, (priority, to))| Goal {
            key: Key::Attack(region),
            kind: "ATTACK_REGION",
            priority,
            cost: 0.0,
            work: Work::Force {
                region,
                to,
                attack: true,
                strength: wanted(ego, picture, region, true),
            },
        })
        .collect()
}

/// A goal for each region holding its things, weighed by the most
/// valuable of them and the threat to it.
fn defend(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let ego = planner.ego();
    let world = picture.world;
    let d = &ego.goal_engine.combat.defend_priority;
    let mut most: BTreeMap<usize, f32> = BTreeMap::new();
    let own = (picture.buildings.iter().map(|&b| Piece::Building(b))).chain(
        picture
            .organizations
            .iter()
            .map(|&r| Piece::Organization(r)),
    );
    for piece in own {
        let Some(region) = picture.region(picture.position(piece)) else {
            continue;
        };
        let value = match piece {
            Piece::Organization(r) => world.full_value(r),
            Piece::Building(b) => world.target_value(Target::Building(b)),
        };
        let templates = picture.templates(piece);
        let worth = liking::worth(ego, picture, &templates, picture.unpacks(piece), value)
            + liking::priority(ego, picture, &templates, true);
        let entry = most.entry(region).or_insert(0.0);
        *entry = entry.max(worth);
    }
    let seen_units = world.sees_units(picture.player);
    let mut out = Vec::new();
    for (region, worth) in most {
        let enemy = picture.enemy[region] + picture.enemy_static[region];
        let mut priority = libm::powf(worth.max(0.0), d.economic_value_exponent)
            * d.economic_value_multiplier
            + libm::powf(enemy, d.enemy_influence_exponent) * d.enemy_influence_multiplier
            + libm::powf(
                picture.enemy_border[region],
                d.enemy_border_influence_exponent,
            ) * d.enemy_border_influence_multiplier;
        let centre = world.regions.regions[region].centre;
        let threatened = (0..world.units.len()).any(|u| {
            let unit = &world.units[u];
            seen_units[u]
                && unit.alive()
                && picture.enemy_of(world.organizations[unit.organization].owner)
                && crate::distance(unit.position, centre) <= d.enemy_forces_threat_distance
        });
        if threatened {
            priority *= d.enemy_forces_bonus;
        }
        let friendly = picture.friendly[region] + picture.friendly_static[region];
        if enemy > 0.0 && (friendly <= 0.0 || enemy >= d.capture_risk_ratio * friendly) {
            priority *= d.capture_risk_bonus;
        }
        out.push(Goal {
            key: Key::Defend(region),
            kind: "DEFEND_REGION",
            priority,
            cost: 0.0,
            work: Work::Force {
                region,
                to: centre,
                attack: false,
                strength: wanted(ego, picture, region, false),
            },
        });
    }
    out
}

/// A goal for each hurt or shaken organization, to take it back into
/// supply.
fn recover(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let rec = planner.ego().goal_engine.recover.clone();
    let mut out = Vec::new();
    for &r in &picture.organizations {
        if !picture.fights(r) {
            continue;
        }
        let under_way = planner.serving_of(r) == Some(&Key::Recover(r));
        let retreating = planner.retreating.contains(&r);
        let health = picture.health(r);
        let morale = picture.morale(r);
        let mut priority: f32 = 0.0;
        if health < rec.max_hp || ((under_way || retreating) && health < 1.0) {
            priority =
                priority.max(rec.base_priority_hp * libm::powf(1.0 - health, rec.exponent_hp));
        }
        if morale < rec.max_morale || ((under_way || retreating) && morale < 1.0) {
            priority = priority
                .max(rec.base_priority_morale * libm::powf(1.0 - morale, rec.exponent_morale));
        }
        if priority <= 0.0 {
            continue;
        }
        let order = recover_order(planner, picture, r);
        out.push(Goal {
            key: Key::Recover(r),
            kind: "RECOVER",
            priority,
            cost: 0.0,
            work: Work::Unit {
                organization: r,
                order,
            },
        });
    }
    out
}

/// Points tried round each supply provider for a unit to recover at.
const RECOVER_CANDIDATES: usize = 5;

/// The share of a provider's supply range those points lie within.
const RECOVER_REACH: f32 = 0.9;

/// What a point to recover at scores before its distance, the most its
/// distance takes off, and the most chance adds.
const RECOVER_BASE: f32 = 201.0;
const RECOVER_FARTHEST: f32 = 200.0;
const RECOVER_FUZZ: f32 = 40.0;

/// Where the organization at `r` goes to recover: nowhere if it's in
/// supply and nothing's fighting it, the place it chose before while that
/// holds, else the best of points round supply providers that are in its
/// side's supply. With none, it routs if it's fighting, else stops.
fn recover_order(planner: &mut Planner, picture: &Picture, r: usize) -> Option<Command> {
    let world = picture.world;
    let o = &world.organizations[r];
    let fought = !o.engaged.is_empty();
    if !fought && world.supplied(picture.player, world.centre(r)) {
        planner.recovering_at.remove(&r);
        return None;
    }
    let kept = (planner.recovering_at.get(&r).copied())
        .filter(|&at| !fought && world.supplied(picture.player, at));
    // Leaving a fight, it's measured from where the fight was.
    let from = match planner.serving_of(r) {
        Some(Key::Attack(g) | Key::Defend(g)) => world.regions.regions[*g].centre,
        _ => world.centre(r),
    };
    let chosen = kept.or_else(|| {
        let providers: Vec<(usize, f32)> = (0..world.buildings.len())
            .filter(|&b| {
                let building = &world.buildings[b];
                building.standing()
                    && !picture.enemy_of(building.owner)
                    && matches!(
                        world.sides.relation(picture.player, building.owner),
                        tgmap::Relation::Itself | tgmap::Relation::Ally
                    )
            })
            .filter_map(|b| Some((b, world.supply_range(b)?)))
            .collect();
        let mut best: Option<(f32, [f32; 2])> = None;
        for (b, range) in providers {
            let around = world.buildings[b].position;
            for _ in 0..RECOVER_CANDIDATES {
                let a = planner.chance() * std::f32::consts::TAU;
                let d = planner.chance() * RECOVER_REACH * range;
                let at = [around[0] + d * libm::cosf(a), around[1] + d * libm::sinf(a)];
                if !world.supplied(picture.player, at) {
                    continue;
                }
                let score = RECOVER_BASE - crate::distance(from, at).min(RECOVER_FARTHEST)
                    + planner.fuzz(RECOVER_FUZZ);
                if score > 0.0 && best.is_none_or(|(s, _)| score > s) {
                    best = Some((score, at));
                }
            }
        }
        best.map(|(_, at)| at)
    });
    match chosen {
        Some(to) => {
            planner.recovering_at.insert(r, to);
            let headed = crate::distance(world.destination(r), to) <= STILL_HEADED;
            (!headed || fought).then_some(Command::Run {
                organization: r,
                to,
            })
        }
        None if fought => Some(Command::Rout { organization: r }),
        None => Some(Command::Stop { organization: r }),
    }
}

/// The mood's deep explore goals, each sent to a region it knows least
/// about.
fn explore(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let world = picture.world;
    if world.fog.setting == tgmap::FogSetting::None {
        return Vec::new();
    }
    let kind = planner.ego().goal_engine.exploration.deep_goals.clone();
    let island = world.regions.main_island().to_vec();
    if island.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for n in 0..kind.num_goals {
        let priority = (kind.base_priority - kind.repeat_penalty * n as f32).max(kind.min_priority);
        let before = planner.exploring.get(&n).copied();
        let mut candidates: Vec<usize> = (0..EXPLORE_CANDIDATES)
            .map(|_| island[(planner.chance() * island.len() as f32) as usize % island.len()])
            .collect();
        candidates.extend(before);
        let mut best: Option<(f32, usize)> = None;
        for region in candidates {
            let centre = world.regions.regions[region].centre;
            let mut score = match world.seen(picture.player, centre) {
                Seen::Unexplored => 2.0,
                Seen::Explored => 1.0,
                Seen::Visible => 0.0,
            };
            if before == Some(region) && score > 0.0 {
                score += 1.0;
            }
            score += planner.chance();
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, region));
            }
        }
        let Some((_, region)) = best else {
            continue;
        };
        planner.exploring.insert(n, region);
        out.push(Goal {
            key: Key::Explore(n),
            kind: "EXPLORE",
            priority,
            cost: 0.0,
            work: Work::Explore {
                to: world.regions.regions[region].centre,
            },
        });
    }
    out
}

/// The reactions between plans: pulling back from losing fights and
/// setting units on enemies near the fights they were sent to.
pub(super) fn react(planner: &mut Planner, world: &World, player: u32) -> Vec<Command> {
    let ego = planner.ego().clone();
    let reacting = &ego.reactive_ai;
    let mut orders = Vec::new();
    let owned = |r: usize| {
        let o = &world.organizations[r];
        o.owner == player && !o.arriving && !world.destroyed(r) && world.computer_may_order(r)
    };
    let enemy = |r: usize| {
        let o = &world.organizations[r];
        !o.arriving
            && !world.destroyed(r)
            && world.sides.relation(player, o.owner) == tgmap::Relation::Enemy
    };
    // Retreat from fights it's losing.
    let mut fighting: Vec<usize> = (planner.serving.keys())
        .filter_map(|m| match *m {
            Member::Organization(r) => Some(r),
            Member::AttackGroup(_) => None,
        })
        .filter(|&r| owned(r))
        .collect();
    fighting.sort_unstable();
    for &r in &fighting {
        let o = &world.organizations[r];
        if o.engaged.is_empty() || planner.retreating.contains(&r) {
            continue;
        }
        let (have, most) = world.survivors(r).fold((0.0, 0.0), |(h, m), u| {
            (h + world.units[u].health, m + world.max_health(u))
        });
        let health = if most > 0.0 { have / most } else { 1.0 };
        if health >= reacting.unit_retreat_max_health {
            continue;
        }
        let theirs: f32 = (o.engaged.iter())
            .map(|p| match *p {
                Piece::Organization(e) => world.full_value(e),
                Piece::Building(b) => world.target_value(Target::Building(b)),
            })
            .sum();
        let ours = world.full_value(r);
        if theirs > 0.0
            && ours / theirs < reacting.unit_retreat_cv_ratio
            && planner.chance() < reacting.unit_retreat_probability
        {
            planner.retreating.insert(r);
            let to = (0..world.buildings.len())
                .filter(|&b| {
                    let building = &world.buildings[b];
                    building.owner == player
                        && building.standing()
                        && world.supply_range(b).is_some()
                })
                .map(|b| world.buildings[b].position)
                .min_by(|a, b| {
                    let from = world.centre(r);
                    crate::distance(*a, from).total_cmp(&crate::distance(*b, from))
                });
            let run = to.map(|to| Command::Run {
                organization: r,
                to,
            });
            orders.extend(run.filter(|c| world.check(player, c).is_ok()));
        }
    }
    // Back to the fight once healed.
    let critical = ego.goal_engine.recover.critical_morale;
    planner.retreating.retain(|&r| {
        if !owned(r) {
            return false;
        }
        let o = &world.organizations[r];
        let morale = if o.morale_max > 0.0 {
            o.morale / o.morale_max
        } else {
            1.0
        };
        let (have, most) = world.survivors(r).fold((0.0, 0.0), |(h, m), u| {
            (h + world.units[u].health, m + world.max_health(u))
        });
        let health = if most > 0.0 { have / most } else { 1.0 };
        morale <= critical || health < reacting.unit_retreat_max_health
    });
    // Targets near the fights units were sent to.
    let seen = world.sees_units(player);
    for &r in &fighting {
        let fights = matches!(planner.serving_of(r), Some(Key::Attack(_) | Key::Defend(_)));
        let o = &world.organizations[r];
        if !fights || planner.retreating.contains(&r) || !o.engaged.is_empty() {
            continue;
        }
        if matches!(o.order, Order::Pursue(_)) {
            continue;
        }
        let region = match planner.serving_of(r) {
            Some(Key::Attack(g) | Key::Defend(g)) => *g,
            _ => continue,
        };
        let steps = world.regions.steps(region);
        let near = |e: usize| {
            let at = world.centre(e);
            (world.regions.at(at))
                .and_then(|g| steps[g])
                .is_some_and(|s| s <= reacting.attack_distance)
        };
        let from = world.centre(r);
        let target = (0..world.organizations.len())
            .filter(|&e| enemy(e) && world.survivors(e).any(|u| seen[u]) && near(e))
            .filter(|&e| world.organizations[e].unpacks.is_none() || world.full_value(e) > 0.0)
            .min_by(|&a, &b| {
                crate::distance(world.centre(a), from)
                    .total_cmp(&crate::distance(world.centre(b), from))
            });
        if let Some(enemy) = target {
            let pursue = Command::Pursue {
                organization: r,
                enemy,
            };
            if world.check(player, &pursue).is_ok() {
                orders.push(pursue);
            }
        }
    }
    orders
}
