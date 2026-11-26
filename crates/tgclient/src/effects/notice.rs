//! Noticing what happens in the sim that plays an effect: shots,
//! landings, health crossing a threshold, states entered and left, and
//! things made.

use bevy::prelude::*;
use tgdata::{HealthCue, HealthTrigger, Id, VfxCue, Weapon};
use tgrender::MapObject;
use tgsim::{Landing, Operated, State, Target, World};

use crate::Sim;
use crate::views::{BuildingView, UnitView};

use super::{Effects, Scenery, Source};

/// What a view's thing was last seen doing, to notice the events of.
#[derive(Component, Default)]
pub(super) struct Seen {
    pub(super) shot: Option<(u64, usize)>,
    /// A share of its maximum.
    pub(super) health: f32,
    pub(super) states: Vec<&'static str>,
    /// As a building, it's been seen finished.
    pub(super) finished: bool,
    /// The properties it was seen holding that show effects.
    pub(super) held: Vec<Id>,
    /// As a site, the tick its health last changed.
    pub(super) worked: Option<u64>,
    /// As a site, whether it was being worked on at its last look, which
    /// it takes once a second.
    pub(super) at_work: bool,
    /// The second of the match it last looked in, as a site.
    pub(super) looked: Option<u64>,
}

/// When something happened: in seconds played and the sim's ticks.
#[derive(Clone, Copy)]
pub(super) struct When {
    pub(super) at: f64,
    pub(super) tick: u64,
    /// For an attack, the index of its weapon.
    pub(super) weapon: Option<usize>,
    /// For a projectile, where it landed; for a special operation, its
    /// centre.
    pub(super) landed: Option<[f32; 2]>,
}

/// What a thing is doing now, to compare with what it was [`Seen`] doing.
pub(super) struct Now<'a> {
    pub(super) states: Vec<&'static str>,
    pub(super) health: f32,
    pub(super) shot: Option<(u64, usize)>,
    pub(super) weapon: Option<&'a Weapon>,
    pub(super) target: Option<Target>,
    /// It's a building that's done going up and is still there.
    pub(super) finished: bool,
    /// It's a site still going up.
    pub(super) site: bool,
    pub(super) held: Vec<Id>,
}

/// The state a site is in while it's being worked on.
pub(super) const AT_WORK: &str = "active_construction";
/// The kind of effect that sinks a site into the ground by the share of
/// its health it's yet to gain.
pub(super) const RAISE: &str = "VFXCONSTRUCTIONRAISEMODIFIER";

pub(super) const DEAD: &str = "dead";

/// The states a building is in, as effect commands name them.
fn building_states(b: &tgsim::Building) -> Vec<&'static str> {
    let mut states = Vec::new();
    if b.packing.is_some() {
        states.push("structure_packing");
    }
    if b.unpacking.is_some() {
        states.push("structure_unpacking");
    }
    if b.constructing && !b.gone() {
        states.push("construction");
    }
    if matches!(b.state, State::Destroyed(_)) {
        states.push(DEAD);
    }
    states
}

/// The properties a unit holds while it lives; a regiment's leader plays
/// those its regiment holds, as the regiment's.
pub(super) fn held(sim: &World, unit: usize) -> Vec<Id> {
    if !sim.units[unit].alive() {
        return Vec::new();
    }
    let organization = &sim.organizations[sim.units[unit].organization];
    let leads = organization.leader.map(|p| organization.units[p]) == Some(unit);
    let regiments = (organization.properties.iter()).filter(|_| leads);
    let mut out: Vec<Id> = Vec::new();
    for p in sim.lasting(unit).chain(regiments.map(|h| &h.property)) {
        if !out.contains(p) {
            out.push(p.clone());
        }
    }
    out
}

/// The properties a building holds for a time while it stands.
pub(super) fn building_held(b: &tgsim::Building) -> Vec<Id> {
    match b.gone() {
        true => Vec::new(),
        false => b.properties.iter().map(|h| h.property.clone()).collect(),
    }
}

/// `health` as a share of `max`.
pub(super) fn share(health: f32, max: f32) -> f32 {
    match max > 0.0 {
        true => (health / max).max(0.0),
        false => 1.0,
    }
}

/// The states entered and those left, going from `before` to `after`.
pub(super) fn changes<'a>(
    before: &'a [&'static str],
    after: &'a [&'static str],
) -> (Vec<&'static str>, Vec<&'static str>) {
    let entered = after.iter().filter(|s| !before.contains(s)).copied();
    let left = before.iter().filter(|s| !after.contains(s)).copied();
    (entered.collect(), left.collect())
}

pub(super) fn by_state(cues: &[(Id, VfxCue)], state: &str) -> Vec<VfxCue> {
    cues.iter()
        .filter(|(s, _)| s.as_str().eq_ignore_ascii_case(state))
        .map(|(_, c)| c.clone())
        .collect()
}

/// The health commands a change of health from `before` to `after`, both
/// shares of the maximum, runs: those whose threshold it crosses, and with
/// any of them, the relative ones too.
pub(super) fn crossed(cues: &[HealthCue], before: f32, after: f32) -> Vec<VfxCue> {
    let runs = |h: &HealthCue| match h.trigger {
        HealthTrigger::Below => before > h.health && after <= h.health,
        HealthTrigger::Above => before <= h.health && after > h.health,
        HealthTrigger::Decrease => before - after > h.health,
        HealthTrigger::Increase => after - before > h.health,
    };
    let absolute = |h: &HealthCue| matches!(h.trigger, HealthTrigger::Below | HealthTrigger::Above);
    let any_absolute = cues.iter().any(|h| absolute(h) && runs(h));
    cues.iter()
        .filter(|h| runs(h) || (any_absolute && !absolute(h)))
        .map(|h| h.cue.clone())
        .collect()
}

/// Whether a command played every `every` shots plays on the shot that
/// many shots have come before.
pub(super) fn on_shot(every: u32, before: u32) -> bool {
    before.is_multiple_of(every.max(1))
}

/// Queues the effect commands of what units and buildings did since last
/// seen, and of scenery for being there.
#[allow(clippy::too_many_arguments)]
pub(super) fn notice(
    mut commands: Commands,
    sim: Res<Sim>,
    time: Res<Time>,
    mut effects: ResMut<Effects>,
    mut sounds: ResMut<crate::sound::Sounds>,
    mut units: Query<(Entity, &UnitView, Option<&mut Seen>)>,
    mut buildings: Query<(Entity, &BuildingView, Option<&mut Seen>), Without<UnitView>>,
    scenery: Query<(Entity, &MapObject), NewScenery>,
) {
    let sim = &sim.0;
    let at = When {
        at: time.elapsed_secs_f64(),
        tick: sim.ticks,
        weapon: None,
        landed: None,
    };
    let fresh = |l: &&Landing| effects.landed.is_none_or(|t| l.tick > t);
    let landings: Vec<Landing> = sim.landings.iter().filter(fresh).cloned().collect();
    for l in &landings {
        if let Some(w) = fighter(sim, l.from).and_then(|f| f.weapons.get(l.weapon)) {
            effects.land(l, w, at.at);
            if let Some(p) = &w.projectile {
                let to = l.target.map_or(l.at, |t| sim.position(t));
                sounds.landed(&time, p, l.at, to, sim.owner(l.from));
            }
        }
    }
    if let Some(l) = landings.last() {
        effects.landed = Some(l.tick);
    }
    let fresh = |o: &&Operated| effects.operated.is_none_or(|t| o.tick > t);
    let operated: Vec<Operated> = sim.operated.iter().filter(fresh).cloned().collect();
    for o in &operated {
        effects.operate(o, at.at);
        if let Some(cues) = effects.operations.get(&o.operation) {
            match o.at {
                Some(at) => sounds.shot(&time, &cues.sounds, at, at, o.player),
                None => sounds.unplaced(&time, &cues.sounds, o.player),
            }
        }
    }
    if let Some(o) = operated.last() {
        effects.operated = Some(o.tick);
    }
    for (e, view, seen) in &mut units {
        let unit = &sim.units[view.unit];
        let fighter = unit.fighter.as_ref();
        let now = Now {
            states: sim.unit_states(view.unit),
            health: share(unit.health, sim.max_health(view.unit)),
            shot: unit.shot,
            weapon: unit.shot.and_then(|(_, w)| fighter?.weapons.get(w)),
            target: unit.target,
            finished: false,
            site: false,
            held: held(sim, view.unit),
        };
        let me = Target::Unit(view.unit);
        see(
            &mut commands,
            &mut effects,
            e,
            seen,
            me,
            unit.template.as_str(),
            now,
            at,
        );
    }
    for (e, view, seen) in &mut buildings {
        let b = &sim.buildings[view.building];
        let now = Now {
            states: building_states(b),
            health: share(b.health, b.fighter.health * b.health_factor),
            shot: b.shot,
            weapon: b.shot.and_then(|(_, w)| b.fighter.weapons.get(w)),
            target: b.target,
            finished: b.unpacking.is_none() && !b.constructing && !b.gone(),
            site: b.constructing && !b.gone(),
            held: building_held(b),
        };
        let me = Target::Building(view.building);
        see(
            &mut commands,
            &mut effects,
            e,
            seen,
            me,
            b.template.as_str(),
            now,
            at,
        );
    }
    for (e, object) in &scenery {
        effects.create(Source::Scenery(e), &object.0.template, at);
        commands.entity(e).insert(Scenery);
    }
}

/// The map's own scenery not yet seen.
type NewScenery = (Without<Scenery>, crate::fog::IsScenery);

/// What a target is, whether it's living or not.
pub(super) fn fighter(sim: &World, t: Target) -> Option<&tgdata::Fighter> {
    match t {
        Target::Unit(u) => sim.units[u].fighter.as_ref(),
        Target::Building(b) => Some(&sim.buildings[b].fighter),
    }
}

/// Witnesses what a view's thing did, seeing it first at the health it has,
/// in no state and unfired.
#[allow(clippy::too_many_arguments)]
pub(super) fn see(
    commands: &mut Commands,
    effects: &mut Effects,
    e: Entity,
    seen: Option<Mut<Seen>>,
    me: Target,
    template: &str,
    now: Now,
    at: When,
) {
    match seen {
        Some(mut seen) => effects.witness(me, template, &mut seen, now, at),
        None => {
            // Health a map stores, or that it was raised with, is no change.
            let mut seen = Seen {
                shot: now.shot,
                health: now.health,
                ..Seen::default()
            };
            effects.create(Source::Thing(me), template, at);
            effects.witness(me, template, &mut seen, now, at);
            commands.entity(e).insert(seen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_regiments_leader_plays_what_the_regiment_holds() {
        let mut w = crate::orders::testing::fresh();
        w.organizations[0].properties.push(tgsim::Held {
            property: Id::from("resupplied"),
            until: 100,
            by: None,
        });
        w.organizations[0].leader = Some(0);
        let leader = w.organizations[0].units[0];
        assert_eq!(held(&w, leader), [Id::from("resupplied")]);
        w.organizations[0].leader = None;
        assert!(held(&w, leader).is_empty());
    }
}
