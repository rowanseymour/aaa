//! Bombard goals: one on each enemy thing it knows of that's worth
//! shelling, carried out by a bombarder already in reach of it.

use super::air::fighting;
use super::fighting::target_worth;
use super::picture::Picture;
use super::planner::{Goal, Key, Planner, Work};

pub(super) fn goals(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let world = picture.world;
    // Nothing to look for without anything that bombards.
    if !(picture.organizations.iter()).any(|&r| world.bombards_at_all(r)) {
        return Vec::new();
    }
    let ego = planner.ego();
    let bombard = &ego.goal_engine.combat.bombardement;
    let mut out = Vec::new();
    for known in &picture.known {
        let Some(region) = picture.region(known.at) else {
            continue;
        };
        // What it can't see, it weighs as in no fight.
        let fights = known.seen && fighting(world, known.piece);
        // A unit's shelled only while it's fighting, a building any time.
        if matches!(known.piece, crate::Piece::Organization(_)) && !fights {
            continue;
        }
        let mut priority = target_worth(ego, picture, known.piece);
        if !fights && !planner.carried_out(&Key::Attack(region)) {
            priority *= bombard.unengaged_target_multiplier;
        }
        if priority <= 0.0 {
            continue;
        }
        out.push(Goal {
            key: Key::Bombard(known.piece),
            kind: "BOMBARD",
            priority,
            cost: 0.0,
            work: Work::Bombard {
                target: known.piece,
                at: known.at,
                seen: known.seen,
            },
        });
    }
    out
}
