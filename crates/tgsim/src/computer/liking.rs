//! How much a computer player's mood likes things: their worth, its
//! `[ActorPriority]` for them and goals' preferences for units.

use tgmap::Id;
use tgmap::sai::{ActorPreference, ActorPriority, Ego};

use super::picture::Picture;
use crate::{Piece, World};

/// Whether something of `template` has the property `property`.
pub(super) fn has(world: &World, template: &Id, property: &Id) -> bool {
    (world.rules.kinds.get(template)).is_some_and(|k| {
        k.properties
            .iter()
            .any(|p| p.as_str().eq_ignore_ascii_case(property.as_str()))
    })
}

/// Whether any of `templates` is `actor` or has `property`, as an entry
/// naming one or the other asks.
fn matches(world: &World, templates: &[&Id], actor: Option<&Id>, property: Option<&Id>) -> bool {
    templates.iter().any(|t| {
        actor.is_some_and(|a| a.as_str().eq_ignore_ascii_case(t.as_str()))
            || property.is_some_and(|p| has(world, t, p))
    })
}

/// How many things of its own have `property` or are of one of
/// `templates`.
pub(super) fn owned(picture: &Picture, templates: &[&Id], property: Option<&Id>) -> usize {
    let world = picture.world;
    let organizations = (picture.organizations.iter()).map(|&r| Piece::Organization(r));
    let buildings = picture.buildings.iter().map(|&b| Piece::Building(b));
    let own = organizations.chain(buildings);
    own.filter(|&p| {
        let theirs = picture.templates(p);
        match property {
            Some(property) => theirs.iter().any(|t| has(world, t, property)),
            None => theirs.first().is_some_and(|t| templates.first() == Some(t)),
        }
    })
    .count()
}

/// What the mood's `[ActorPriority]` makes of a thing made of `templates`,
/// its own first: the best of the entries that apply, each its value less
/// its repeat penalty for each the side has, plus its one-time bonus if it
/// has none. For a thing that's `existing`, one fewer is counted and
/// there's no one-time bonus.
pub(super) fn priority(ego: &Ego, picture: &Picture, templates: &[&Id], existing: bool) -> f32 {
    let world = picture.world;
    let entries = &ego.goal_engine.actor_priority;
    (entries.iter())
        .filter(|e| {
            matches(
                world,
                templates,
                e.actor_ids.as_ref(),
                e.property_ids.as_ref(),
            )
        })
        .map(|e| entry_priority(e, picture, templates, existing))
        .max_by(f32::total_cmp)
        .unwrap_or(0.0)
}

fn entry_priority(e: &ActorPriority, picture: &Picture, templates: &[&Id], existing: bool) -> f32 {
    let property = e
        .property_ids
        .as_ref()
        .filter(|_| e.count_all_property_actors);
    let mut n = owned(picture, templates, property);
    if existing {
        n = n.saturating_sub(1);
    }
    let once = match n == 0 && !existing {
        true => e.one_time_bonus,
        false => 0.0,
    };
    e.value - e.repeat_penalty * n as f32 + once
}

/// The most of a kind of thing, by `templates`, it recruits in one think.
pub(super) fn most_per_think(ego: &Ego, world: &World, templates: &[&Id]) -> u32 {
    let engine = &ego.goal_engine;
    let set = (engine.actor_priority.iter())
        .filter(|e| {
            matches(
                world,
                templates,
                e.actor_ids.as_ref(),
                e.property_ids.as_ref(),
            )
        })
        .map(|e| e.max_recruited_per_think)
        .find(|&m| m > 0);
    set.unwrap_or(engine.default_max_recruited_per_think)
}

/// A thing's worth, by `templates` (what it unpacks into, if anything,
/// among them): what it makes and uses of each resource, by how much the
/// side needs it, and its strength, scaled.
pub(super) fn worth(
    ego: &Ego,
    picture: &Picture,
    templates: &[&Id],
    unpacks: Option<&Id>,
    combat_value: f32,
) -> f32 {
    let v = &ego.goal_engine.actor_value;
    let own: Vec<&Id> = (templates.iter().copied())
        .filter(|t| Some(*t) != unpacks)
        .collect();
    let balance = &picture.balance;
    let mut economic = economic(ego, picture.world, balance, &own);
    // A packed building counts what it unpacks into, if that's worth more.
    if let Some(building) = unpacks {
        economic = economic.max(self::economic(ego, picture.world, balance, &[building]));
    }
    (economic + combat_value * v.military_cv_mult) * v.scaling_mult
}

/// A thing of `template`'s worth to a side whose economy stands at
/// `balance`.
pub(super) fn worth_to(
    ego: &Ego,
    world: &World,
    balance: &crate::Balance,
    template: &Id,
    combat_value: f32,
) -> f32 {
    let v = &ego.goal_engine.actor_value;
    let economic = economic(ego, world, balance, &[template]);
    (economic + combat_value * v.military_cv_mult) * v.scaling_mult
}

/// What `templates`' production less upkeep is worth to a side whose
/// economy stands at `balance`.
fn economic(ego: &Ego, world: &World, balance: &crate::Balance, templates: &[&Id]) -> f32 {
    let mut made = crate::economy::Amounts::default();
    for t in templates {
        if let Some(e) = world.rules.economies.get(*t) {
            for (r, a) in e.production.iter() {
                made.set(r.as_str(), made.get(r.as_str()) + a);
            }
            for (r, a) in e.upkeep.iter() {
                made.set(r.as_str(), made.get(r.as_str()) - a);
            }
        }
    }
    made_worth(ego, world, balance, &made)
}

/// What a side whose economy stands at `balance` makes of gaining `made`
/// a minute.
pub(super) fn made_worth(
    ego: &Ego,
    world: &World,
    balance: &crate::Balance,
    made: &crate::economy::Amounts,
) -> f32 {
    let rules = &world.rules.economy;
    let management = &ego.goal_engine.resource_management;
    let mut out = 0.0;
    for (resource, amount) in made.nonzero() {
        let stockpiled = rules.stockpiled().any(|s| s == resource);
        let now = balance.net.get(resource.as_str());
        let need = management.need(resource.as_str(), stockpiled);
        out += need.worth(now, now + amount);
    }
    out
}

/// What goal kind `kind`'s preferences make of the organization at `r`:
/// the value they add, and what they multiply by.
pub(super) fn preference(ego: &Ego, world: &World, kind: &str, r: usize) -> (f32, f32) {
    let Some(weights) = ego.goal_engine.weights(kind) else {
        return (0.0, 1.0);
    };
    let o = &world.organizations[r];
    let members: Vec<&Id> = world
        .survivors(r)
        .map(|u| &world.units[u].template)
        .collect();
    let (mut value, mut multiplier) = (0.0, 1.0);
    for p in &weights.actor_preference {
        let fits = |t: &Id| matches(world, &[t], p.actor_ids.as_ref(), p.property_ids.as_ref());
        let own = fits(&o.template);
        let count = members.iter().filter(|t| fits(t)).count();
        if !own && count == 0 {
            continue;
        }
        value += per_company(p, own, count) * p.value;
        multiplier *= p.multiplier;
    }
    (value, multiplier)
}

/// How many times a preference's value counts for one company.
fn per_company(p: &ActorPreference, own: bool, members: usize) -> f32 {
    match p.count_value_once_per_company || own {
        true => 1.0,
        false => members as f32,
    }
}

/// What travelling `seconds` on average to a goal leaves of its worth.
pub(super) fn travel_factor(ego: &Ego, seconds: f32) -> f32 {
    let t = &ego.goal_engine.travel_time;
    let share = (1.0 - 0.8 * seconds / t.max_travel_time.max(f32::MIN_POSITIVE)).max(0.2);
    libm::powf(share, t.travel_time_exponent)
}

/// Seconds the organization at `r` takes to reach `to`, over the regions'
/// travel distances.
pub(super) fn travel_time(picture: &Picture, r: usize, to: usize) -> f32 {
    let world = picture.world;
    let Some(from) = picture.region(world.centre(r)) else {
        return f32::INFINITY;
    };
    let distance = picture.travel(from)[to];
    distance / world.organizations[r].speed.max(0.1)
}
