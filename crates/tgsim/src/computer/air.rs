//! Air attack goals: one on each enemy thing it knows of, carried out by
//! an airfield's attack group.

use tgmap::AirJob;

use super::fighting::target_worth;
use super::picture::Picture;
use super::planner::{Goal, Key, Planner, Work};
use crate::{Piece, SECOND, Target, World};

pub(super) fn goals(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    if !planner.sends_air_attacks() || airfields(picture).is_empty() {
        return Vec::new();
    }
    let world = picture.world;
    planner.struck.retain(|_, &mut until| world.ticks < until);
    let ego = planner.ego();
    let air = &ego.goal_engine.combat.air_attacks;
    let mut out = Vec::new();
    for known in &picture.known {
        let Some(region) = picture.region(known.at) else {
            continue;
        };
        if planner.struck.contains_key(&known.piece) || aloft(world, known.piece) {
            continue;
        }
        // What it can't see, it weighs as it last saw it: in no fight, whole.
        let fighting = known.seen && fighting(world, known.piece);
        let health = match known.seen {
            true => health(picture, known.piece),
            false => 1.0,
        };
        let mut priority = target_worth(ego, picture, known.piece);
        let mut bonus = false;
        if fighting {
            priority *= air.engaged_bonus;
            bonus = true;
        }
        if (air.target_damage_floor..air.target_damage_ceiling).contains(&health) {
            priority *= air.target_damage_bonus;
            bonus = true;
        }
        if health < air.absolute_min_health {
            priority *= match fighting {
                true => air.absolute_min_health_engaged_mult,
                false => air.absolute_min_health_unengaged_mult,
            };
            bonus = true;
        }
        if known.seen && resupplying(picture, known.piece) {
            priority *= air.resupplying_mult;
            bonus = true;
        }
        let supporting = planner.carried_out(&Key::Attack(region));
        if supporting {
            priority *= air.support_ground_attack_bonus;
        } else if !bonus {
            priority *= air.no_bonus_penalty;
        }
        if priority <= 0.0 {
            continue;
        }
        out.push(Goal {
            key: Key::AirAttack(known.piece),
            kind: "AIR_ATTACK",
            priority,
            cost: 0.0,
            work: Work::AirAttack {
                target: known.piece,
                at: known.at,
                seen: known.seen,
            },
        });
    }
    out
}

/// The tick until which a thing an air attack is sent at now isn't
/// attacked from the air again.
pub(super) fn struck_until(planner: &Planner, world: &World) -> u64 {
    let air = &planner.ego().goal_engine.combat.air_attacks;
    world.ticks + (air.min_time_between_attacks.max(0.0) * SECOND as f32) as u64
}

/// Whether a thing is in the air, out of a bomber's reach.
fn aloft(world: &World, piece: Piece) -> bool {
    match piece {
        Piece::Organization(r) => world.survivors(r).any(|u| world.units[u].in_air()),
        Piece::Building(_) => false,
    }
}

/// Whether a thing is in a fight: an organization engaging or engaged, a
/// building shooting at something or under attack.
fn fighting(world: &World, piece: Piece) -> bool {
    match piece {
        Piece::Organization(r) => world.fighting(r),
        Piece::Building(b) => {
            let shooting = match world.buildings[b].target {
                Some(Target::Unit(u)) => world.units[u].alive(),
                Some(Target::Building(t)) => world.buildings[t].standing(),
                None => false,
            };
            shooting
                || (0..world.organizations.len())
                    .any(|e| world.organizations[e].engaged.contains(&piece) && !world.destroyed(e))
        }
    }
}

/// A thing's share of its health left.
fn health(picture: &Picture, piece: Piece) -> f32 {
    let world = picture.world;
    match piece {
        Piece::Organization(r) => picture.health(r),
        Piece::Building(b) => {
            let most = world.building_max_health(b);
            match most > 0.0 {
                true => world.buildings[b].health / most,
                false => 1.0,
            }
        }
    }
}

/// Whether a thing is being made good: an organization hurt and in
/// supply.
fn resupplying(picture: &Picture, piece: Piece) -> bool {
    match piece {
        Piece::Organization(r) => {
            picture.world.organizations[r].in_supply && picture.health(r) < 1.0
        }
        Piece::Building(_) => false,
    }
}

/// The buildings of its side that can send air attacks, each with its
/// attack group's speed.
pub(super) fn airfields(picture: &Picture) -> Vec<(usize, f32)> {
    let world = picture.world;
    (picture.buildings.iter().copied())
        .filter_map(|b| {
            let group = world.airfield(b)?.group(AirJob::Offensive)?;
            let speeds = (group.templates.iter())
                .filter_map(|t| world.rules.raisable.get(t))
                .flat_map(|o| o.members.iter().map(|m| m.speed));
            Some((b, speeds.fold(f32::INFINITY, f32::min)))
        })
        .map(|(b, speed)| (b, if speed.is_finite() { speed } else { 0.0 }))
        .collect()
}

/// Whether the building at `b`'s attack is still out, its planes not all
/// home.
pub(super) fn attack_out(world: &World, b: usize) -> bool {
    (world.sorties.iter())
        .any(|s| s.airfield == b && matches!(s.job, crate::Job::Attack(_)) && !s.stage.over())
}
