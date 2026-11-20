//! Plays the visual effects the game data gives objects as things happen to
//! them: dust while buildings pack and unpack, flashes and hit puffs as
//! they fire, explosions where shells land, and smoke, fire and debris as
//! they're hurt. Which of an object's effect commands an event runs, where
//! the effect goes and how long it lasts are the data's; this notices the
//! events in the sim and plays the effects drawn by a model and those that
//! are pictures laid on the ground.

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::prelude::*;
use tgck::map::CELL_SIZE;
use tgdata::{
    Decal, GameData, HealthCue, HealthTrigger, Id, ObjectEffects, OperationCues, Shape, Targets,
    Vfx, VfxCue, VfxTint, Weapon,
};
use tgmap::{Ground, Map};
use tgrender::camera::RtsCamera;
use tgrender::decals::{DecalCache, decal_colour};
use tgrender::models::{Footprint, Models, Rate};
use tgrender::terrain::{map_point, world};
use tgrender::{MapAssets, MapObject, Tint};
use tgsim::{Landing, Operated, State, Target, World};

use crate::{BuildingView, Drawn, Loaded, Screen, Sim, UnitView};

#[derive(Resource)]
pub struct Effects {
    vfx: Arc<HashMap<Id, Vfx>>,
    objects: HashMap<Id, ObjectEffects>,
    /// What plays as each special operation is used, by id.
    operations: HashMap<Id, OperationCues>,
    /// What each property shows on what gains it, by id.
    properties: HashMap<Id, Vec<VfxCue>>,
    /// The node names of the model points commands name, by id.
    bones: HashMap<Id, String>,
    /// What the pictures laid on the ground so far are drawn with.
    decals: DecalCache,
    /// Each map tile's terrain group, row-major, `side` tiles a row.
    groups: Vec<Option<Id>>,
    side: usize,
    /// Commands waiting to play, with when.
    queued: Vec<(f64, Play)>,
    /// Shots each weapon has fired, by who fired it and the weapon's index.
    shots: HashMap<(Target, usize), u32>,
    /// The tick of the last projectile landing seen.
    landed: Option<u64>,
    /// The tick of the last special operation seen carried out.
    operated: Option<u64>,
    /// How far up an attacker a hit on its target lands, as a share of
    /// the attacker's height.
    aim: f32,
    /// The effects detail setting, from 0 for none to 1 for all, which
    /// effects that aren't critical play by.
    detail: f32,
    /// The map's sun is dark enough that effects for the night show.
    night: bool,
    /// Seconds since a site's health last changed that it's still being
    /// worked on, if the game says.
    at_work: Option<f32>,
    rng: u64,
}

/// An effect command set off, waiting to play.
struct Play {
    cue: VfxCue,
    source: Source,
    /// What the event was aimed at, if anything.
    target: Option<Target>,
    /// The sim tick it happened on.
    tick: u64,
    /// For a projectile, where it landed; for a special operation, its
    /// centre.
    landed: Option<[f32; 2]>,
    /// For an attack, the index of the weapon among its attacker's.
    weapon: Option<usize>,
    /// The property it shows on what holds it, so that losing the property
    /// stops only what it started.
    holding: Option<Id>,
}

/// What set off an effect command.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    Thing(Target),
    /// The projectile of a shot fired by this, which has reached its
    /// target.
    Projectile(Target),
    /// A piece of the map's scenery, by its view.
    Scenery(Entity),
    /// A special operation, on the ground at its centre.
    Operation,
}

/// Something an effect plays on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Subject {
    Thing(Target),
    Scenery(Entity),
}

/// Scenery whose effects for being created are queued.
#[derive(Component)]
struct Scenery;

/// What a view's thing was last seen doing, to notice the events of.
#[derive(Component, Default)]
struct Seen {
    shot: Option<(u64, usize)>,
    /// A share of its maximum.
    health: f32,
    states: Vec<&'static str>,
    /// As a building, it's been seen finished.
    finished: bool,
    /// The properties it was seen holding that show effects.
    held: Vec<Id>,
    /// As a site, the tick its health last changed.
    worked: Option<u64>,
    /// As a site, whether it was being worked on at its last look, which
    /// it takes once a second.
    at_work: bool,
    /// The second of the match it last looked in, as a site.
    looked: Option<u64>,
}

/// One of the effects playing.
#[derive(Component)]
struct Effect {
    vfx: Id,
    /// Whose command created it, for a stop command to find it by.
    by: Option<Subject>,
    /// The property it shows, for one shown on what holds it.
    holding: Option<Id>,
    /// When it ends, unless it lasts until stopped.
    ends: Option<f64>,
    /// It goes once its spot is off screen.
    discard: bool,
}

/// An effect that stays where it was put for something that stays put,
/// which goes when that does and is hidden while it is.
#[derive(Component)]
struct Tied(Entity);

/// An effect for the night, hidden while the sun's too bright for it.
#[derive(Component)]
struct NightOnly;

/// An effect a command stopped that's still to go as it does.
#[derive(Component)]
struct Stopped;

/// What an effect that stays where it's put goes with: the building that
/// made it, by its view, for one that lasts until stopped; else what it's
/// attached to.
fn tie(view: Option<Entity>, attached: Option<Entity>, permanent: bool) -> Option<Entity> {
    view.filter(|_| permanent).or(attached)
}

/// A picture laid on the ground that takes time to show or to go.
#[derive(Component)]
struct Fading {
    decal: Decal,
    made: f64,
    /// Seconds it lasts before it starts to go, for one that does.
    life: Option<f32>,
    /// It's shown in full and stays so, with nothing left to change.
    settled: bool,
}

/// Seconds a picture laid on the ground takes to show, given its own time
/// to: none for one whose effect starts the moment the match does.
fn fade_in(own: f32, tick: u64, delay: f32) -> f32 {
    match tick == 0 && delay <= 0.0 {
        true => 0.0,
        false => own,
    }
}

/// How much of a picture laid on the ground shows `age` seconds after it
/// was made: it comes in over `fade`'s first time, and once `life` is up
/// goes over its second, from however far it had come.
fn opacity([rise, fall]: [f32; 2], age: f32, life: Option<f32>) -> f32 {
    let risen = |age: f32| match rise > 0.0 {
        true => (age / rise).clamp(0.0, 1.0),
        false => 1.0,
    };
    match life.filter(|l| age > *l) {
        Some(life) if fall > 0.0 => risen(life) * (1.0 - (age - life) / fall).max(0.0),
        Some(_) => 0.0,
        None => risen(age),
    }
}

/// An attached effect's place on what it's attached to.
#[derive(Component)]
struct Follow {
    anchor: Entity,
    on: OnThing,
    /// It keeps the way it faced when it began.
    gyroscope: bool,
}

/// Where on a thing an effect goes.
#[derive(Clone, Copy)]
struct OnThing {
    /// A point of the thing's model, from its origin in model units.
    point: Transform,
    /// Moved this far, in world units, along the point's axes.
    offset: Vec3,
    /// And turned this way from it.
    turn: Quat,
}

impl OnThing {
    /// Where it is, on a thing placed at `root`; drawn at its own size,
    /// whatever the thing's.
    fn place(&self, root: &Transform) -> Transform {
        let rotation = root.rotation * self.point.rotation;
        Transform {
            translation: root.transform_point(self.point.translation) + rotation * self.offset,
            rotation: rotation * self.turn,
            scale: Vec3::ONE,
        }
    }

    /// Turns it as an effect placed by it facing `was` was turned, to
    /// face `now`, so it goes on facing that way from its thing.
    fn turned(mut self, was: Quat, now: Quat) -> OnThing {
        self.turn = self.turn * was.inverse() * now;
        self
    }
}

impl Effects {
    /// How the tint with this id changes its object's colours.
    pub fn tint(&self, ids: &str) -> Option<VfxTint> {
        self.vfx.get(&Id::from(ids))?.tint
    }

    pub fn new(game: &GameData, map: &Map) -> Effects {
        let vfx = game.vfx();
        let objects = game.object_effects();
        let operations = game.operation_cues();
        let properties = game.property_cues();
        let bones = game.bones();
        let variations = game.terrain_variations();
        let terrain = &map.terrain;
        let groups = terrain
            .tiles
            .iter()
            .map(|t| {
                let name = terrain.name_of(t);
                variations.get(&Id::from(name))?.group.clone()
            })
            .collect();
        let night = game.night(map.sun.diffuse);
        debug!(
            "{} effects shown by {} object templates, at {} named points, by {}",
            vfx.len(),
            objects.len(),
            bones.len(),
            if night { "night" } else { "day" }
        );
        Effects {
            vfx,
            objects,
            operations,
            properties,
            bones,
            decals: DecalCache::default(),
            groups,
            side: terrain.side,
            queued: Vec::new(),
            shots: HashMap::new(),
            landed: None,
            operated: None,
            aim: game.aim_heights().straight,
            detail: 1.0,
            night,
            at_work: game.active_construction_seconds(),
            rng: 0x9e37_79b9_7f4a_7c15,
        }
    }

    /// Has effects for the night show, or not.
    pub(crate) fn set_night(&mut self, night: bool) {
        self.night = night;
    }

    /// The terrain group of the tile at a map point.
    fn group_at(&self, [x, y]: [f32; 2]) -> Option<&Id> {
        if self.side == 0 {
            return None;
        }
        let tile = |w: f32| ((w / CELL_SIZE).floor().max(0.0) as usize).min(self.side - 1);
        self.groups.get(tile(y) * self.side + tile(x))?.as_ref()
    }

    /// A number in `0..1`.
    fn random(&mut self) -> f32 {
        // xorshift64
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    fn queue(&mut self, when: When, cues: &[VfxCue], source: Source, target: Option<Target>) {
        self.queue_holding(when, cues, source, target, None);
    }

    fn queue_holding(
        &mut self,
        when: When,
        cues: &[VfxCue],
        source: Source,
        target: Option<Target>,
        holding: Option<&Id>,
    ) {
        for cue in cues {
            let play = Play {
                cue: cue.clone(),
                source,
                target,
                tick: when.tick,
                landed: when.landed,
                weapon: when.weapon,
                holding: holding.cloned(),
            };
            self.queued.push((when.at + f64::from(cue.delay), play));
        }
    }

    /// Queues the commands of a projectile's landing: those of its
    /// template's for landing, and its weapon's for each target it lands
    /// on.
    fn land(&mut self, landing: &Landing, weapon: &Weapon, at: f64) {
        let when = When {
            at,
            tick: landing.tick,
            weapon: Some(landing.weapon),
            landed: Some(landing.at),
        };
        let target = Some(landing.target);
        let landed = (weapon.projectile.as_ref())
            .and_then(|p| self.objects.get(p))
            .map(|o| o.destroyed.clone())
            .unwrap_or_default();
        self.queue(when, &landed, Source::Projectile(landing.from), target);
        let per_target = weapon.effects.per_target.clone();
        let when = When {
            landed: None,
            ..when
        };
        self.queue(when, &per_target, Source::Thing(landing.from), target);
    }

    /// Queues what plays as a special operation is used, on the ground at
    /// its centre.
    fn operate(&mut self, operated: &Operated, at: f64) {
        let when = When {
            at,
            tick: operated.tick,
            weapon: None,
            landed: operated.at,
        };
        let visuals = (self.operations.get(&operated.operation))
            .map(|c| c.visuals.clone())
            .unwrap_or_default();
        self.queue(when, &visuals, Source::Operation, None);
    }

    /// Queues the commands a thing of `template` has for being created.
    fn create(&mut self, source: Source, template: &str, at: When) {
        if let Some(o) = self.objects.get(&Id::from(template)) {
            let created = o.created.clone();
            self.queue(at, &created, source, None);
        }
    }

    /// Queues the commands of what `me`, of `template`, did since it was
    /// `seen`, and notes what it's doing `now`.
    fn witness(&mut self, me: Target, template: &str, seen: &mut Seen, now: Now, at: When) {
        let objects = self.objects.get(&Id::from(template)).cloned();
        let mut now = now;
        if now.site {
            if now.health != seen.health {
                seen.worked = Some(at.tick);
            }
            let second = (at.tick as f32 * tgsim::TICK) as u64;
            if seen.looked != Some(second) {
                seen.looked = Some(second);
                let ago = |t: u64| (at.tick - t) as f32 * tgsim::TICK;
                let within = |s: f32| seen.worked.is_some_and(|t| ago(t) <= s);
                seen.at_work = self.at_work.is_some_and(within);
            }
            if seen.at_work {
                now.states.push(AT_WORK);
            }
        }
        if let Some(o) = &objects {
            let (entered, left) = changes(&seen.states, &now.states);
            for state in entered {
                let cues = by_state(&o.entered, state);
                self.queue(at, &cues, Source::Thing(me), None);
                if state == DEAD {
                    self.queue(at, &o.destroyed, Source::Thing(me), None);
                }
            }
            for state in left {
                // What lasts on the thing that entering it started ends
                // with it.
                let on_it =
                    |c: &VfxCue| c.attached && matches!(c.at.as_str(), "SELF" | "TARGET_THING");
                let lasting = |c: &VfxCue| {
                    c.duration.is_none()
                        && on_it(c)
                        && self.vfx.get(&c.vfx).is_some_and(|v| v.permanent)
                };
                let ended: Vec<VfxCue> = (by_state(&o.entered, state).into_iter())
                    .filter(|c| !c.stop && lasting(c))
                    .map(|c| VfxCue {
                        stop: true,
                        delay: 0.0,
                        ..c
                    })
                    .collect();
                self.queue(at, &ended, Source::Thing(me), None);
                let cues = by_state(&o.left, state);
                self.queue(at, &cues, Source::Thing(me), None);
            }
            // A site's health going up and down sets nothing off, finishing
            // included.
            let was_site = seen.states.contains(&"construction");
            if now.health != seen.health && !now.site && !was_site {
                let cues = crossed(&o.health, seen.health, now.health);
                self.queue(at, &cues, Source::Thing(me), None);
            }
            if now.finished && !seen.finished {
                self.queue(at, &o.constructed, Source::Thing(me), None);
            }
        }
        if now.shot != seen.shot
            && let (Some((_, w)), Some(weapon)) = (now.shot, now.weapon)
        {
            let fired = self.shots.entry((me, w)).or_default();
            let n = *fired;
            *fired += 1;
            let e = &weapon.effects;
            // A projectile's targets are hit as it lands.
            let per_target = match weapon.flight {
                Some(_) => None,
                None => Some(&e.per_target),
            };
            let attack: Vec<VfxCue> = [Some(&e.start), Some(&e.shot), per_target, Some(&e.end)]
                .into_iter()
                .flatten()
                .flatten()
                .filter(|c| on_shot(c.every, n))
                .cloned()
                .collect();
            let at = When {
                weapon: Some(w),
                ..at
            };
            self.queue(at, &attack, Source::Thing(me), now.target);
        }
        // A property's effects are on what holds it, and go with it.
        let source = Source::Thing(me);
        for p in now.held.iter().filter(|p| !seen.held.contains(p)) {
            let cues = self.properties.get(p).cloned().unwrap_or_default();
            self.queue_holding(at, &cues, source, Some(me), Some(p));
        }
        for p in seen.held.iter().filter(|p| !now.held.contains(p)) {
            // What hasn't started yet never will.
            (self.queued).retain(|(_, q)| q.source != source || q.holding.as_ref() != Some(p));
            let stops: Vec<VfxCue> = (self.properties.get(p).into_iter().flatten())
                .filter(|c| !c.stop)
                .map(|c| VfxCue {
                    stop: true,
                    delay: 0.0,
                    ..c.clone()
                })
                .collect();
            self.queue_holding(at, &stops, source, Some(me), Some(p));
        }
        seen.held = now.held;
        seen.shot = now.shot;
        seen.health = now.health;
        seen.states = now.states;
        seen.finished |= now.finished;
    }
}

/// When something happened: in seconds played and the sim's ticks.
#[derive(Clone, Copy)]
struct When {
    at: f64,
    tick: u64,
    /// For an attack, the index of its weapon.
    weapon: Option<usize>,
    /// For a projectile, where it landed; for a special operation, its
    /// centre.
    landed: Option<[f32; 2]>,
}

/// What a thing is doing now, to compare with what it was [`Seen`] doing.
struct Now<'a> {
    states: Vec<&'static str>,
    health: f32,
    shot: Option<(u64, usize)>,
    weapon: Option<&'a Weapon>,
    target: Option<Target>,
    /// It's a building that's done going up and is still there.
    finished: bool,
    /// It's a site still going up.
    site: bool,
    held: Vec<Id>,
}

/// The state a site is in while it's being worked on.
const AT_WORK: &str = "active_construction";
/// The kind of effect that sinks a site into the ground by the share of
/// its health it's yet to gain.
const RAISE: &str = "VFXCONSTRUCTIONRAISEMODIFIER";

/// On an effect that sinks a building's view, the view, for as long as the
/// effect lasts.
#[derive(Component)]
pub struct Lowers(pub Entity);

const DEAD: &str = "dead";

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

/// The properties a unit holds while it lives.
fn held(sim: &World, unit: usize) -> Vec<Id> {
    match sim.units[unit].alive() {
        true => sim.lasting(unit).cloned().collect(),
        false => Vec::new(),
    }
}

/// `health` as a share of `max`.
fn share(health: f32, max: f32) -> f32 {
    match max > 0.0 {
        true => (health / max).max(0.0),
        false => 1.0,
    }
}

/// The states entered and those left, going from `before` to `after`.
fn changes<'a>(
    before: &'a [&'static str],
    after: &'a [&'static str],
) -> (Vec<&'static str>, Vec<&'static str>) {
    let entered = after.iter().filter(|s| !before.contains(s)).copied();
    let left = before.iter().filter(|s| !after.contains(s)).copied();
    (entered.collect(), left.collect())
}

fn by_state(cues: &[(Id, VfxCue)], state: &str) -> Vec<VfxCue> {
    cues.iter()
        .filter(|(s, _)| s.as_str().eq_ignore_ascii_case(state))
        .map(|(_, c)| c.clone())
        .collect()
}

/// The health commands a change of health from `before` to `after`, both
/// shares of the maximum, runs: those whose threshold it crosses, and with
/// any of them, the relative ones too.
fn crossed(cues: &[HealthCue], before: f32, after: f32) -> Vec<VfxCue> {
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
fn on_shot(every: u32, before: u32) -> bool {
    before.is_multiple_of(every.max(1))
}

/// Whether an effect plays at the effects detail setting `detail`, given
/// `roll`, a number in `0..1`: critical ones always do.
fn detail_allows(critical: bool, detail: f32, roll: f32) -> bool {
    critical || roll < detail
}

/// Whether a command for the terrain group `filter` plays on ground of
/// `group`.
fn terrain_allows(filter: Option<&Id>, group: Option<&Id>) -> bool {
    match filter {
        None => true,
        Some(f) => group.is_some_and(|g| g.as_str().eq_ignore_ascii_case(f.as_str())),
    }
}

/// Seconds an effect lasts, or `None` for one that lasts until stopped: a
/// duration of its command's or its own, else its model's animation once
/// through, which may be no time at all.
fn lifetime(vfx: &Vfx, cue: &VfxCue, animation: Option<f32>) -> Option<f32> {
    if let Some(d) = cue.duration {
        return Some(d);
    }
    if vfx.permanent {
        return None;
    }
    Some(vfx.duration.or(animation).unwrap_or(0.0))
}

/// Whether a point is within the camera's view; true where that can't be
/// told.
fn on_screen(camera: Option<(&Camera, &GlobalTransform)>, at: Vec3) -> bool {
    let Some(ndc) = camera.and_then(|(c, g)| c.world_to_ndc(g, at)) else {
        return true;
    };
    ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z)
}

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (notice, play, follow, show_by_night, fade, tint_units)
                .chain()
                .after(crate::show_packing)
                .after(crate::show_units)
                .after(crate::show_buildings)
                .after(crate::fog::hide_buildings)
                .run_if(in_state(Screen::Match))
                .run_if(resource_exists::<Effects>),
        );
    }
}

/// Queues the effect commands of what units and buildings did since last
/// seen, and of scenery for being there.
#[allow(clippy::too_many_arguments)]
fn notice(
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
                let to = crate::target_position(sim, l.target);
                sounds.landed(&time, p, l.at, to, owner(sim, l.from));
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
            held: Vec::new(),
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

/// Whose a target is.
fn owner(sim: &World, t: Target) -> u32 {
    match t {
        Target::Unit(u) => sim.organizations[sim.units[u].organization].owner,
        Target::Building(b) => sim.buildings[b].owner,
    }
}

/// What a target is, whether it's living or not.
fn fighter(sim: &World, t: Target) -> Option<&tgdata::Fighter> {
    match t {
        Target::Unit(u) => sim.units[u].fighter.as_ref(),
        Target::Building(b) => Some(&sim.buildings[b].fighter),
    }
}

/// Witnesses what a view's thing did, seeing it first at the health it has,
/// in no state and unfired.
#[allow(clippy::too_many_arguments)]
fn see(
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

/// A thing's view as effects find it.
struct Placed<'a> {
    /// What an effect on it follows.
    anchor: Entity,
    /// Where it is.
    root: Transform,
    thing: &'a tgck::map::Thing,
    /// The model it's drawn with: while it packs or unpacks, its packing
    /// model, unpacking if this is true.
    packing: Option<bool>,
    /// The way it faces across the ground, a unit vector.
    facing: Vec2,
    /// What it covers of the ground.
    footprint: Shape,
    /// It was alive when the event happened.
    living: bool,
    /// What it's armed with, if it fights.
    fighter: Option<&'a tgdata::Fighter>,
}

/// Plays the queued commands that are due.
#[allow(clippy::too_many_arguments)]
fn play(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<Sim>,
    mut effects: ResMut<Effects>,
    loaded: Res<Loaded>,
    mut drawn: ResMut<Drawn>,
    mut assets: MapAssets,
    units: Query<(Entity, &UnitView, Option<&Footprint>)>,
    buildings: Query<(Entity, &BuildingView, Option<&Footprint>)>,
    things: Query<(&Transform, &MapObject, Option<&Footprint>)>,
    playing: Query<(Entity, &Effect, Has<Fading>)>,
    cameras: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    fog: Res<crate::fog::Fog>,
) {
    let now = time.elapsed_secs_f64();
    if effects.queued.iter().all(|(due, _)| *due > now) {
        return;
    }
    let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut effects.queued)
        .into_iter()
        .partition(|(due, _)| *due <= now);
    effects.queued = waiting;
    let sim = &sim.0;
    // Each view's anchor, root, packing model and footprint.
    let mut views = HashMap::new();
    for (e, v, footprint) in &units {
        let unit = &sim.units[v.unit];
        let shape = unit.shape.unwrap_or_else(|| rect(footprint));
        views.insert(Target::Unit(v.unit), (e, e, None, shape));
    }
    for (e, v, footprint) in &buildings {
        // While it packs or unpacks it's drawn as its packing model, which
        // is what its effects then play on.
        let (anchor, packing) = match v.packing {
            Some((p, unpacking)) => (p, Some(unpacking)),
            None => (e, None),
        };
        views.insert(
            Target::Building(v.building),
            (anchor, e, packing, rect(footprint)),
        );
    }
    let placed = |s: Subject, tick: u64| {
        let t = match s {
            Subject::Thing(t) => t,
            Subject::Scenery(e) => {
                let (transform, object, footprint) = things.get(e).ok()?;
                let [x, y, _] = object.0.facing;
                return Some(Placed {
                    anchor: e,
                    root: *transform,
                    thing: &object.0,
                    packing: None,
                    facing: Vec2::new(x, y).try_normalize().unwrap_or(Vec2::NEG_Y),
                    footprint: rect(footprint),
                    living: true,
                    fighter: None,
                });
            }
        };
        let &(anchor, root, packing, footprint) = views.get(&t)?;
        let (transform, object, _) = things.get(root).ok()?;
        let (facing, living) = match t {
            Target::Unit(u) => {
                let u = &sim.units[u];
                (u.facing, u.alive() || u.died.is_some_and(|d| d >= tick))
            }
            Target::Building(b) => {
                let b = &sim.buildings[b];
                let fell = matches!(b.state, State::Destroyed(f) if f >= tick);
                (b.facing, b.standing() || fell)
            }
        };
        Some(Placed {
            anchor,
            root: *transform,
            thing: &object.0,
            packing,
            facing: Vec2::from(facing).try_normalize().unwrap_or(Vec2::NEG_Y),
            footprint,
            living,
            fighter: fighter(sim, t),
        })
    };
    let camera = cameras.iter().find(|(c, _)| c.is_active);
    let Loaded { game, map } = &*loaded;
    for (_, p) in due {
        let cue = &p.cue;
        let (by, fired_by) = match p.source {
            Source::Thing(t) => (Some(Subject::Thing(t)), Some(t)),
            Source::Projectile(t) => (None, Some(t)),
            Source::Scenery(e) => (Some(Subject::Scenery(e)), None),
            Source::Operation => (None, None),
        };
        if cue.stop {
            for (e, effect, fades) in &playing {
                let ours = effect.by == by && effect.holding == p.holding;
                if ours && by.is_some() && effect.vfx == cue.vfx {
                    // One that fades goes in its own time.
                    match fades {
                        true => drop(commands.entity(e).insert(Stopped)),
                        false => commands.entity(e).despawn(),
                    }
                }
            }
            continue;
        }
        let Some(vfx) = effects.vfx.get(&cue.vfx).cloned() else {
            continue;
        };
        if vfx.category == RAISE && fits(sim, &cue.targets, fired_by, p.target) {
            let view = match by {
                Some(Subject::Thing(t @ Target::Building(_))) => views.get(&t).map(|v| v.1),
                _ => None,
            };
            if let Some(view) = view {
                commands.spawn((
                    Lowers(view),
                    Effect {
                        vfx: cue.vfx.clone(),
                        by,
                        holding: p.holding.clone(),
                        ends: None,
                        discard: false,
                    },
                    DespawnOnExit(Screen::Match),
                ));
            }
            continue;
        }
        let path = vfx.model.as_deref().filter(|_| vfx.category == "VFXMODEL");
        if path.is_none() && vfx.decal.is_none() {
            continue;
        }
        // One for the night is still made by day, hidden, so as to show
        // if the sun goes down.
        let nightly = (NightOnly, visible(effects.night));
        if !fits(sim, &cue.targets, fired_by, p.target) {
            continue;
        }
        let place = Place {
            placed: &|t| placed(t, p.tick),
            ground: &map.ground,
            bones: &effects.bones,
            aim: effects.aim,
        };
        let spotted = place.spot(&p, &mut drawn.0);
        let Some((mut transform, attach)) = spotted else {
            debug!(
                "{}: nowhere to play at {} {:?}",
                cue.vfx, cue.at, cue.location
            );
            continue;
        };
        let group = effects.group_at(map_point(transform.translation)).cloned();
        if !terrain_allows(cue.terrain.as_ref(), group.as_ref()) {
            continue;
        }
        let roll = effects.random();
        if !detail_allows(vfx.critical, effects.detail, roll) {
            continue;
        }
        let length = path.and_then(|p| drawn.0.effect_timing(p).map(|t| t.length));
        let lasts = lifetime(&vfx, cue, length);
        let discard = lasts.is_some() && !vfx.never_discard;
        if discard && !on_screen(camera, transform.translation) {
            continue;
        }
        let fogged = fog.at(map_point(transform.translation)) != tgsim::Seen::Visible;
        if lasts.is_some() && fogged {
            continue;
        }
        let turn = match vfx.random_orientation {
            true => Quat::from_rotation_y(effects.random() * TAU),
            false => Quat::IDENTITY,
        };
        let unturned = transform.rotation;
        // A model takes only a heading from where it's put, unless it
        // hangs off what it's attached to.
        let hangs = attach.is_some() && !vfx.gyroscope;
        let stood = match path.is_some() && !hangs {
            true => upright(unturned * Vec3::Z),
            false => unturned,
        };
        transform.rotation = turn * stood;
        // The building that made it, if that's still there.
        let view = match by {
            Some(Subject::Thing(t @ Target::Building(b))) if !sim.buildings[b].gone() => {
                views.get(&t).map(|v| v.1)
            }
            _ => None,
        };
        let Some(path) = path else {
            // A picture stays where it's laid, so not one on what moves.
            let moves = matches!(by, Some(Subject::Thing(Target::Unit(_))));
            let Some(decal) = vfx.decal.as_ref().filter(|_| !(moves && attach.is_some())) else {
                continue;
            };
            // One made as the match begins is there in full at once.
            let decal = &Decal {
                fade: [fade_in(decal.fade[0], p.tick, cue.delay), decal.fade[1]],
                ..decal.clone()
            };
            let shown = opacity(decal.fade, 0.0, lasts);
            let cache = &mut effects.decals;
            let laid = assets.decal(game, cache, &map.ground, decal, &transform, shown);
            let Some(laid) = laid else {
                continue;
            };
            debug!(
                "{}: {} at {}",
                cue.vfx, decal.texture, transform.translation
            );
            let mut entity = commands.spawn((
                laid,
                Effect {
                    vfx: cue.vfx.clone(),
                    by,
                    holding: p.holding.clone(),
                    ends: lasts.map(|s| now + f64::from(s + decal.fade[1])),
                    discard: false,
                },
                DespawnOnExit(Screen::Match),
            ));
            if vfx.night_only {
                entity.insert(nightly);
            }
            if decal.fade != [0.0; 2] {
                entity.insert(Fading {
                    decal: decal.clone(),
                    made: now,
                    life: lasts,
                    settled: false,
                });
            }
            let attached = attach.map(|(anchor, _)| anchor);
            if let Some(anchor) = tie(view, attached, lasts.is_none()) {
                entity.insert(Tied(anchor));
            }
            continue;
        };
        let mut models = assets.models();
        let Some(e) = drawn.0.spawn_effect(
            &mut commands,
            &mut models,
            path,
            &vfx.texture_dirs,
            transform,
        ) else {
            continue;
        };
        debug!("{}: {path} at {}", cue.vfx, transform.translation);
        let mut entity = commands.entity(e);
        entity.insert((
            Effect {
                vfx: cue.vfx.clone(),
                by,
                holding: p.holding.clone(),
                ends: lasts.map(|s| now + f64::from(s)),
                discard,
            },
            DespawnOnExit(Screen::Match),
        ));
        if vfx.night_only {
            entity.insert(nightly);
        }
        if let Some(rate) = rate(&vfx, cue, length) {
            entity.insert(Rate(rate));
        }
        if attach.is_none()
            && let Some(anchor) = tie(view, None, lasts.is_none())
        {
            entity.insert(Tied(anchor));
        }
        if let Some((anchor, on)) = attach {
            entity.insert(Follow {
                anchor,
                on: on.turned(unturned, transform.rotation),
                gyroscope: vfx.gyroscope,
            });
        }
    }
}

/// A footprint of `size`, width across the facing and length along it, as
/// a shape; a point without one.
fn rect(size: Option<&Footprint>) -> Shape {
    match size {
        Some(&Footprint(Vec2 { x, y })) => Shape::Rect {
            width: x,
            height: y,
        },
        None => Shape::Point,
    }
}

/// How fast an effect given a duration plays its model, so that its
/// animation plays once over it; `None` for its own pace.
fn rate(vfx: &Vfx, cue: &VfxCue, length: Option<f32>) -> Option<f32> {
    let duration = cue.duration.or(vfx.duration)?;
    let length = length.filter(|l| *l > 0.0)?;
    (duration > 0.0).then_some(length / duration)
}

/// How far out from its centre a footprint's edge is, going in `dir` from
/// it, for one facing `facing`; both unit vectors across the ground.
fn edge(footprint: Shape, facing: Vec2, dir: Vec2) -> f32 {
    match footprint {
        Shape::Point => 0.0,
        Shape::Circle { radius } => radius,
        Shape::Rect { width, height } => {
            let along = dir.dot(facing).abs();
            let across = dir.perp_dot(facing).abs();
            let reach = |half: f32, part: f32| match part > 1e-6 {
                true => half / part,
                false => f32::INFINITY,
            };
            reach(width / 2.0, across).min(reach(height / 2.0, along))
        }
    }
}

/// How high above where its target stands a hit lands: `ratio` of the way
/// up the attacker's model, held within the target's; with no attacker,
/// that far up the target's. Each model's extent is its bottom and top.
fn hit_height(ratio: f32, attacker: Option<[f32; 2]>, target: [f32; 2]) -> f32 {
    let up = |[bottom, top]: [f32; 2]| bottom + ratio * (top - bottom);
    let [bottom, top] = target;
    attacker
        .map_or(up(target), up)
        .clamp(bottom, top.max(bottom))
}

/// What placing an effect needs to know.
struct Place<'a, 'b> {
    placed: &'b dyn Fn(Subject) -> Option<Placed<'a>>,
    ground: &'b Ground,
    bones: &'b HashMap<Id, String>,
    /// How far up an attacker a hit lands.
    aim: f32,
}

impl Place<'_, '_> {
    /// Where a command's effect goes, and for an attached one, what it
    /// follows and where on it; `None` if what it's on is gone, or it's
    /// attached to a point the model doesn't have.
    fn spot(
        &self,
        play: &Play,
        models: &mut Models,
    ) -> Option<(Transform, Option<(Entity, OnThing)>)> {
        let Play {
            cue,
            source,
            target,
            landed,
            ..
        } = play;
        let at = cue.at.as_str();
        let offset = world(cue.translation[0], cue.translation[1], cue.translation[2]);
        let turn = Quat::from_rotation_arc(Vec3::Z, facing(cue.orientation));
        let on_ground = |[x, y]: [f32; 2]| {
            let mut t = Transform::from_translation(world(x, y, self.ground.height_at([x, y])));
            t.translation += offset;
            t.rotation = turn;
            t
        };
        let (attacker, own) = match *source {
            Source::Thing(t) => (Some(Subject::Thing(t)), Some(Subject::Thing(t))),
            Source::Projectile(t) => (Some(Subject::Thing(t)), None),
            Source::Scenery(e) => (None, Some(Subject::Scenery(e))),
            Source::Operation => (None, None),
        };
        match at {
            "MAP_CENTER" => {
                let c = self.ground.size() / 2.0;
                return Some((on_ground([c, c]), None));
            }
            // A projectile is where it landed, on its own.
            "SELF" | "SOURCE_GROUND" if own.is_none() => {
                return Some((on_ground((*landed)?), None));
            }
            // An operation's others are on the ground at its centre.
            _ if *source == Source::Operation => return Some((on_ground((*landed)?), None)),
            _ => {}
        }
        let subject = match at {
            "TARGET_THING" | "TARGET_GROUND" | "TARGET_HIT_POSITION" => Subject::Thing((*target)?),
            "SOURCE_ABILITY_POSITION" => attacker?,
            _ => own?,
        };
        let thing = (self.placed)(subject)?;
        let here = map_point(thing.root.translation);
        let extent = |models: &mut Models, p: &Placed| {
            let path = models.model_path(p.thing, p.packing)?.to_string();
            let [bottom, top] = models.extent(&path)?;
            let scale = p.root.scale.y;
            Some([bottom * scale, top * scale])
        };
        match at {
            "SOURCE_GROUND" | "TARGET_GROUND" => Some((on_ground(here), None)),
            "TARGET_HIT_POSITION" => {
                if !thing.living {
                    return None;
                }
                let target_extent = extent(models, &thing)?;
                let by = attacker.and_then(self.placed);
                let across = match &by {
                    Some(a) => {
                        let dir = -a.facing;
                        Vec2::from(here) + dir * edge(thing.footprint, thing.facing, dir)
                    }
                    None => Vec2::from(here),
                };
                let height = match by {
                    Some(a) => hit_height(self.aim, extent(models, &a), target_extent),
                    None => hit_height(self.aim, None, target_extent),
                };
                let base = thing.root.translation.y;
                let mut t = Transform::from_translation(world(across.x, across.y, base + height));
                t.translation += offset;
                t.rotation = turn;
                Some((t, None))
            }
            "SOURCE_ABILITY_POSITION" => {
                let base = thing.root.translation.y;
                let weapons = thing.fighter.map_or(&[][..], |f| &f.weapons[..]);
                let weapon = weapons.get(play.weapon.unwrap_or(0));
                let turret = weapon.and_then(|w| w.turret_point.as_ref());
                if let Some(point) = turret.and_then(|id| self.point(models, &thing, id.as_str())) {
                    let on = OnThing {
                        point,
                        offset,
                        turn,
                    };
                    return Some((on.place(&thing.root), None));
                }
                let on_thing = |at: Vec3| {
                    let at = thing.root.translation + thing.root.rotation * (at + offset);
                    Transform::from_translation(at).with_rotation(thing.root.rotation * turn)
                };
                match weapon {
                    Some(w) if w.projectile.is_some() => {
                        let [x, y, z] = w.spawn_position;
                        Some((on_thing(world(x, y, z)), None))
                    }
                    Some(_) => {
                        let ahead = thing.facing;
                        let reach = edge(thing.footprint, thing.facing, ahead);
                        let front = Vec2::from(here) + ahead * reach;
                        let height = thing.fighter.map_or(0.0, |f| f.attack_height);
                        let at = world(front.x, front.y, base + height);
                        let mut t = Transform::from_translation(at + offset);
                        t.rotation = turn;
                        Some((t, None))
                    }
                    None => Some((on_thing(Vec3::ZERO), None)),
                }
            }
            _ => {
                let point = match &cue.location {
                    Some(id) => match (self.point(models, &thing, id), cue.attached) {
                        (Some(p), _) => p,
                        // Detached, it plays as though it had no location.
                        (None, false) => Transform::IDENTITY,
                        (None, true) => return None,
                    },
                    None => Transform::IDENTITY,
                };
                let on = OnThing {
                    point,
                    offset,
                    turn,
                };
                let transform = on.place(&thing.root);
                let attach = cue.attached.then_some((thing.anchor, on));
                Some((transform, attach))
            }
        }
    }

    /// Where the point a command names by `id` is in the model `thing` is
    /// drawn with now, if it has it.
    fn point(&self, models: &mut Models, thing: &Placed, id: &str) -> Option<Transform> {
        let name = self.bones.get(&Id::from(id)).map_or(id, |n| n);
        let path = models.model_path(thing.thing, thing.packing)?.to_string();
        models.point(&path, name)
    }
}

/// The way a command's orientation, in the game's axes, points an effect:
/// the default, the -y its models face along, leaves it as it is.
fn facing([x, y, z]: [f32; 3]) -> Vec3 {
    // The game's -y is Bevy's +z.
    world(x, y, z).try_normalize().unwrap_or(Vec3::Z)
}

/// The turn that points a model along `heading` the right way up; none
/// for a heading straight up or down, which says nothing of which way.
fn upright(heading: Vec3) -> Quat {
    match heading.cross(Vec3::Y).length_squared() > 1e-6 {
        // A model's forward is +z, where looking is along -z.
        true => Transform::IDENTITY.looking_to(-heading, Vec3::Y).rotation,
        false => Quat::IDENTITY,
    }
}

/// What [`follow`] reads and moves of an effect.
type Playing<'a> = (
    Entity,
    &'a mut Effect,
    Option<&'a Follow>,
    Option<&'a Tied>,
    Option<&'a mut Fading>,
    Has<Stopped>,
    Has<NightOnly>,
    &'a mut Transform,
    &'a mut Visibility,
);

/// Moves attached effects with what they're on, and ends those whose time
/// is up, whose thing has gone, or that went off screen.
fn follow(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<Sim>,
    mut effects: Query<Playing>,
    lit: Res<Effects>,
    anchors: Query<(&Transform, &Visibility, Option<&BuildingView>), Without<Effect>>,
    cameras: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
) {
    let now = time.elapsed_secs_f64();
    let camera = cameras.iter().find(|(c, _)| c.is_active);
    // A building's hidden once it's gone, though its view stays.
    let there = |anchor: Entity| {
        (anchors.get(anchor).ok())
            .filter(|(_, v, building)| !(building.is_some() && **v == Visibility::Hidden))
    };
    // Fog hides a building that's still there, so that's asked of the sim.
    let stays = |anchor: Entity| {
        (anchors.get(anchor).ok())
            .filter(|(_, _, b)| b.is_none_or(|v| !sim.0.buildings[v.building].gone()))
    };
    for (e, mut effect, follow, tied, fading, stopped, nightly, mut transform, mut visibility) in
        &mut effects
    {
        let with = |shown: &Visibility| shown_with(*shown, nightly, lit.night);
        if effect.ends.is_some_and(|t| t <= now) {
            commands.entity(e).despawn();
            continue;
        }
        let orphaned = tied.is_some_and(|t| stays(t.0).is_none());
        if stopped || orphaned {
            // A picture with a time to go in goes over it, from however
            // much of it shows.
            match fading.filter(|f| f.decal.fade[1] > 0.0) {
                Some(mut f) => {
                    (f.life, f.settled) = (Some((now - f.made) as f32), false);
                    effect.ends = Some(now + f64::from(f.decal.fade[1]));
                    commands.entity(e).remove::<(Tied, Stopped)>();
                }
                None => commands.entity(e).despawn(),
            }
            continue;
        }
        if let Some((_, shown, _)) = tied.and_then(|t| stays(t.0)) {
            visibility.set_if_neq(with(shown));
        }
        // Scenery's effects go when it's cleared, and are hidden with it
        // on ground never seen.
        if let Some(Subject::Scenery(s)) = effect.by {
            let Ok((_, shown, _)) = anchors.get(s) else {
                commands.entity(e).despawn();
                continue;
            };
            visibility.set_if_neq(with(shown));
        }
        if let Some(f) = follow {
            let Some((root, _, building)) = there(f.anchor) else {
                commands.entity(e).despawn();
                continue;
            };
            // What's on a site stays on the ground as its model's sunk.
            let standing = building.filter(|v| sim.0.buildings[v.building].constructing);
            let root = match standing {
                Some(v) => &Transform {
                    translation: v.placed,
                    ..*root
                },
                None => root,
            };
            let placed = f.on.place(root);
            transform.translation = placed.translation;
            if !f.gyroscope {
                transform.rotation = placed.rotation;
            }
        }
        if effect.discard && !on_screen(camera, transform.translation) {
            trace!("{}: off screen", effect.vfx);
            commands.entity(e).despawn();
        }
    }
}

/// Shows effects for the night only while it's night. Those shown with
/// what they're on are left to `follow`.
fn show_by_night(
    effects: Res<Effects>,
    mut nightly: Query<(&Effect, &mut Visibility, Has<Tied>), With<NightOnly>>,
) {
    for (effect, mut visibility, tied) in &mut nightly {
        if tied || matches!(effect.by, Some(Subject::Scenery(_))) {
            continue;
        }
        visibility.set_if_neq(visible(effects.night));
    }
}

/// How an effect shows with what it's on, `shown` as that is: hidden with
/// it, and by day if it's `nightly`.
fn shown_with(shown: Visibility, nightly: bool, night: bool) -> Visibility {
    match shown {
        Visibility::Hidden => Visibility::Hidden,
        _ if nightly && !night => Visibility::Hidden,
        _ => Visibility::Inherited,
    }
}

/// How an effect for the night shows, by whether it's night.
fn visible(night: bool) -> Visibility {
    match night {
        true => Visibility::Inherited,
        false => Visibility::Hidden,
    }
}

/// Shows pictures laid on the ground as far as they've come in or gone.
fn fade(
    time: Res<Time>,
    mut laid: Query<(&mut Fading, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let now = time.elapsed_secs_f64();
    for (mut fading, material) in &mut laid {
        if fading.settled {
            continue;
        }
        let age = (now - fading.made) as f32;
        let shown = opacity(fading.decal.fade, age, fading.life);
        if fading.life.is_none() && age >= fading.decal.fade[0] {
            fading.settled = true;
        }
        let colour = decal_colour(&fading.decal, shown);
        if materials
            .get(&material.0)
            .is_some_and(|m| m.base_color != colour)
            && let Some(mut m) = materials.get_mut(&material.0)
        {
            m.base_color = colour;
        }
    }
}

/// Whether a command naming `targets` plays when `by` sets it off on `on`:
/// any does that names no flags.
fn fits(sim: &World, targets: &Targets, by: Option<Target>, on: Option<Target>) -> bool {
    match (by, on) {
        _ if targets.required.is_empty() && targets.excluded.is_empty() => true,
        (Some(by), Some(on)) => sim.matches(by, targets, on),
        _ => false,
    }
}

/// A tint a property shows: its effect, its pulse, and whether it runs on
/// the wall clock rather than game time.
type Shown<'a> = (&'a Id, VfxTint, bool);

impl Effects {
    /// The tints the properties `held` show on `me`, in order.
    fn tints(&self, sim: &World, me: Target, held: &[Id]) -> Vec<Shown<'_>> {
        (held.iter())
            .flat_map(|p| self.properties.get(p).into_iter().flatten())
            .filter(|c| !c.stop && fits(sim, &c.targets, Some(me), Some(me)))
            .filter_map(|c| {
                let v = self.vfx.get(&c.vfx)?;
                Some((&c.vfx, v.tint?, v.real_time))
            })
            .collect()
    }
}

/// On a unit tinted for the properties it holds: when each of their tints
/// began, on the clock it runs on, so that one a stronger tint hid carries
/// on its pulse once that goes.
#[derive(Component, Default)]
struct Tinting {
    since: HashMap<Id, f64>,
}

impl Tinting {
    /// The colour of the first of `tints` with the highest priority, at
    /// `clocks`' game and real times.
    fn colour(&mut self, tints: &[Shown], [game, real]: [f64; 2]) -> Option<[f32; 3]> {
        let now = |real_time| if real_time { real } else { game };
        (self.since).retain(|id, _| tints.iter().any(|(t, ..)| *t == id));
        for &(id, _, real_time) in tints {
            self.since.entry(id.clone()).or_insert(now(real_time));
        }
        let &(id, pulse, real_time) = (tints.iter().rev()).max_by_key(|(_, t, _)| t.priority)?;
        let steps = crate::supply::TINT_STEPS;
        let age = (now(real_time) - self.since[id]) as f32;
        let level = (pulse.level(age) * steps).round() / steps;
        Some(pulse.colour(level))
    }
}

/// Tints each unit for the properties it holds, and plain again at once
/// when it holds none that tint.
#[allow(clippy::type_complexity)]
fn tint_units(
    mut commands: Commands,
    sim: Res<Sim>,
    effects: Res<Effects>,
    real: Res<Time<Real>>,
    mut views: Query<(
        Entity,
        &UnitView,
        &Seen,
        Option<&mut Tinting>,
        Option<&Tint>,
    )>,
) {
    let sim = &sim.0;
    let clocks = [
        sim.ticks as f64 * f64::from(tgsim::TICK),
        real.elapsed_secs_f64(),
    ];
    for (e, view, seen, tinting, tint) in &mut views {
        let tints = effects.tints(sim, Target::Unit(view.unit), &seen.held);
        let tinted = tinting.is_some() || tint.is_some();
        let colour = match tinting {
            Some(mut t) => t.colour(&tints, clocks),
            None => {
                let mut t = Tinting::default();
                let colour = t.colour(&tints, clocks);
                if colour.is_some() {
                    commands.entity(e).insert(t);
                }
                colour
            }
        };
        match colour.map(Tint) {
            Some(colour) if tint != Some(&colour) => drop(commands.entity(e).insert(colour)),
            Some(_) => {}
            None if tinted => drop(commands.entity(e).remove::<(Tinting, Tint)>()),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tgdata::Targets;

    fn cue(vfx: &str) -> VfxCue {
        VfxCue {
            vfx: Id::from(vfx),
            stop: false,
            at: "SELF".into(),
            location: None,
            attached: true,
            translation: [0.0; 3],
            orientation: [0.0, -1.0, 0.0],
            duration: None,
            delay: 0.0,
            every: 1,
            terrain: None,
            targets: Targets::default(),
            stop_on_destructed: true,
            chain: Vec::new(),
            chain_immediate: Vec::new(),
        }
    }

    fn health(trigger: HealthTrigger, health: f32, vfx: &str) -> HealthCue {
        HealthCue {
            health,
            trigger,
            cue: cue(vfx),
        }
    }

    fn ids(cues: &[VfxCue]) -> Vec<&str> {
        cues.iter().map(|c| c.vfx.as_str()).collect()
    }

    #[test]
    fn health_effects_play_on_crossing_their_thresholds() {
        let cues = [
            health(HealthTrigger::Below, 0.4, "smoke"),
            health(HealthTrigger::Below, 0.0, "debris"),
            health(HealthTrigger::Above, 0.5, "mended"),
        ];
        assert!(crossed(&cues, 1.0, 0.6).is_empty());
        assert_eq!(ids(&crossed(&cues, 0.6, 0.4)), ["smoke"]);
        assert_eq!(ids(&crossed(&cues, 1.0, 0.0)), ["smoke", "debris"]);
        // Below one already, it doesn't play again until it's back above.
        assert!(crossed(&cues, 0.4, 0.3).is_empty());
        assert_eq!(ids(&crossed(&cues, 0.3, 0.6)), ["mended"]);
        assert_eq!(ids(&crossed(&cues, 0.6, 0.3)), ["smoke"]);
    }

    #[test]
    fn relative_health_effects_play_on_big_changes_and_with_the_rest() {
        let cues = [
            health(HealthTrigger::Below, 0.5, "smoke"),
            health(HealthTrigger::Decrease, 0.3, "jolt"),
            health(HealthTrigger::Increase, 0.3, "patched"),
        ];
        assert_eq!(ids(&crossed(&cues, 1.0, 0.6)), ["jolt"]);
        assert!(crossed(&cues, 1.0, 0.8).is_empty());
        assert_eq!(ids(&crossed(&cues, 0.6, 0.5)), ["smoke", "jolt", "patched"]);
        assert_eq!(ids(&crossed(&cues, 0.2, 0.6)), ["patched"]);
    }

    #[test]
    fn a_sites_lasting_effects_end_with_it_and_work_on_it_puffs_dust() {
        let site = ObjectEffects {
            entered: vec![
                (Id::from("construction"), cue("scaffold")),
                (Id::from(AT_WORK), cue("dust")),
            ],
            left: vec![(Id::from("construction"), cue("teardown"))],
            ..ObjectEffects::default()
        };
        let mut effects = effects(HashMap::from([(Id::from("bunker"), site)]));
        effects.vfx = Arc::new(HashMap::from([(Id::from("scaffold"), vfx(true, None))]));
        let mut seen = Seen {
            health: 0.1,
            ..Seen::default()
        };
        let me = Target::Building(0);
        let second = (1.0 / tgsim::TICK) as u64;
        // Healed in its first second, left alone for three, then finished.
        let looks = [
            (0, 0.1, true),
            (1, 0.2, true),
            (second, 0.2, true),
            (4 * second, 0.2, true),
            (5 * second, 1.0, false),
        ];
        let mut played = Vec::new();
        for (tick, health, site) in looks {
            let now = Now {
                states: if site {
                    vec!["construction"]
                } else {
                    Vec::new()
                },
                health,
                shot: None,
                weapon: None,
                target: None,
                finished: !site,
                site,
                held: Vec::new(),
            };
            effects.witness(me, "bunker", &mut seen, now, at(tick));
            let queued = std::mem::take(&mut effects.queued);
            let cues = queued
                .iter()
                .map(|(_, p)| (p.cue.vfx.to_string(), p.cue.stop));
            played.push(cues.collect::<Vec<_>>());
        }
        let s = |v: &str, stop| (v.to_string(), stop);
        assert_eq!(played[0], [s("scaffold", false)]);
        assert_eq!(played[2], [s("dust", false)]);
        assert_eq!(played[3], []);
        assert_eq!(played[4], [s("scaffold", true), s("teardown", false)]);
    }

    #[test]
    fn states_entered_and_left() {
        let (entered, left) = changes(&["structure_unpacking"], &["dead"]);
        assert_eq!((entered, left), (vec!["dead"], vec!["structure_unpacking"]));
        let (entered, left) = changes(&["dead"], &["dead"]);
        assert!(entered.is_empty() && left.is_empty());
    }

    #[test]
    fn a_frequency_plays_on_the_first_shot_and_every_nth_after() {
        let plays: Vec<u32> = (0..7).filter(|&n| on_shot(3, n)).collect();
        assert_eq!(plays, [0, 3, 6]);
        assert!((0..4).all(|n| on_shot(1, n)));
    }

    #[test]
    fn detail_thins_out_effects_that_arent_critical() {
        assert!(detail_allows(false, 1.0, 0.999));
        assert!(!detail_allows(false, 0.0, 0.0));
        assert!(detail_allows(false, 0.6, 0.5));
        assert!(!detail_allows(false, 0.6, 0.7));
        assert!(detail_allows(true, 0.0, 0.7));
    }

    #[test]
    fn effects_that_last_go_with_the_building_that_made_them() {
        let mut world = bevy::ecs::world::World::new();
        let (view, model) = (world.spawn_empty().id(), world.spawn_empty().id());
        // One that lasts goes with its building, attached to it or not.
        assert_eq!(tie(Some(view), None, true), Some(view));
        assert_eq!(tie(Some(view), Some(model), true), Some(view));
        // One with its own time is left to it, unless it's attached.
        assert_eq!(tie(Some(view), None, false), None);
        assert_eq!(tie(Some(view), Some(model), false), Some(model));
        // With no building still there, only what it's attached to.
        assert_eq!(tie(None, Some(model), true), Some(model));
        assert_eq!(tie(None, None, true), None);
    }

    #[test]
    fn pictures_on_the_ground_come_in_and_go_over_their_fade_times() {
        // In over 10 seconds, staying for good.
        assert_eq!(opacity([10.0, 1.0], 0.0, None), 0.0);
        assert_eq!(opacity([10.0, 1.0], 5.0, None), 0.5);
        assert_eq!(opacity([10.0, 1.0], 60.0, None), 1.0);
        // There at once for 15 seconds, then gone over 5 more.
        assert_eq!(opacity([0.0, 5.0], 15.0, Some(15.0)), 1.0);
        assert_eq!(opacity([0.0, 5.0], 17.5, Some(15.0)), 0.5);
        assert_eq!(opacity([0.0, 5.0], 21.0, Some(15.0)), 0.0);
        // Ended half way in, it goes from there.
        assert_eq!(opacity([10.0, 4.0], 6.0, Some(5.0)), 0.375);
        // With no time to go in, it's gone when its time's up.
        assert_eq!(opacity([0.0, 0.0], 3.0, Some(2.0)), 0.0);
        assert_eq!(opacity([0.0, 0.0], 3.0, None), 1.0);
        // Only what starts with the match skips coming in.
        assert_eq!(fade_in(10.0, 0, 0.0), 0.0);
        assert_eq!(fade_in(10.0, 0, 0.5), 10.0);
        assert_eq!(fade_in(10.0, 3, 0.0), 10.0);
    }

    #[test]
    fn terrain_filters_match_their_group() {
        let snow = Id::from("snow");
        assert!(terrain_allows(None, None));
        assert!(terrain_allows(Some(&snow), Some(&Id::from("Snow"))));
        assert!(!terrain_allows(Some(&snow), Some(&Id::from("dirt"))));
        assert!(!terrain_allows(Some(&snow), None));
    }

    fn vfx(permanent: bool, duration: Option<f32>) -> Vfx {
        Vfx {
            category: "VFXMODEL".into(),
            model: None,
            texture_dirs: Vec::new(),
            permanent,
            critical: false,
            duration,
            attached_scale: 0.0,
            never_discard: false,
            night_only: false,
            constant_screen_size: false,
            real_time: false,
            random_orientation: false,
            gyroscope: true,
            tint: None,
            decal: None,
        }
    }

    #[test]
    fn effects_last_their_duration_else_their_animation() {
        let flash = cue("flash");
        assert_eq!(lifetime(&vfx(false, None), &flash, Some(0.5)), Some(0.5));
        assert_eq!(lifetime(&vfx(false, None), &flash, None), Some(0.0));
        assert_eq!(
            lifetime(&vfx(false, Some(2.0)), &flash, Some(0.5)),
            Some(2.0)
        );
        assert_eq!(lifetime(&vfx(true, None), &flash, Some(0.5)), None);
        let timed = VfxCue {
            duration: Some(3.0),
            ..flash
        };
        assert_eq!(lifetime(&vfx(true, Some(2.0)), &timed, None), Some(3.0));
    }

    #[test]
    fn effects_play_on_a_things_point_at_their_own_size() {
        let root = Transform::from_xyz(10.0, 0.0, -5.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
            .with_scale(Vec3::splat(2.0));
        let on = OnThing {
            point: Transform::from_xyz(0.0, 1.0, 1.0),
            offset: Vec3::ZERO,
            turn: Quat::IDENTITY,
        };
        let t = on.place(&root);
        assert!(t.translation.distance(Vec3::new(12.0, 2.0, -5.0)) < 1e-5);
        assert_eq!(t.scale, Vec3::ONE);
        assert_eq!(facing([0.0, -1.0, 0.0]), Vec3::Z);
    }

    #[test]
    fn a_turned_effect_keeps_its_turn_from_a_thing_that_turns() {
        let root = Transform::from_rotation(Quat::from_rotation_y(1.0));
        let on = OnThing {
            point: Transform::from_rotation(Quat::from_rotation_x(0.3)),
            offset: Vec3::ZERO,
            turn: Quat::from_rotation_z(0.2),
        };
        let was = on.place(&root).rotation;
        let now = Quat::from_rotation_y(2.0) * was;
        let on = on.turned(was, now);
        let same = |a: Quat, b: Quat| {
            [Vec3::X, Vec3::Y]
                .iter()
                .all(|&v| (a * v).distance(b * v) < 1e-5)
        };
        assert!(same(on.place(&root).rotation, now));
        assert!(!same(was, now));
        // Its thing turning turns it as far.
        let further = Quat::from_rotation_y(0.5);
        let root = Transform::from_rotation(further * root.rotation);
        assert!(same(on.place(&root).rotation, further * now));
    }

    #[test]
    fn models_stand_upright_along_their_heading() {
        // On a point lying on its side, turned a quarter turn about.
        use std::f32::consts::FRAC_PI_2;
        let lying = Quat::from_rotation_y(FRAC_PI_2) * Quat::from_rotation_z(FRAC_PI_2);
        let stood = upright(lying * Vec3::Z);
        assert!((stood * Vec3::Z).distance(Vec3::X) < 1e-5);
        assert!((stood * Vec3::Y).distance(Vec3::Y) < 1e-5);
        // Pitched with a heading up a slope, but not rolled.
        let up = Vec3::new(0.0, 1.0, 1.0).normalize();
        let stood = upright(up);
        assert!((stood * Vec3::Z).distance(up) < 1e-5);
        assert!((stood * Vec3::X).y.abs() < 1e-5);
        assert_eq!(upright(Vec3::Y), Quat::IDENTITY);
    }

    #[test]
    fn effects_for_the_night_show_only_while_it_is_night() {
        let mut app = App::new();
        app.insert_resource(effects(HashMap::new()))
            .add_systems(Update, show_by_night);
        let effect = || Effect {
            vfx: Id::from("lit_window"),
            by: None,
            holding: None,
            ends: None,
            discard: false,
        };
        let world = app.world_mut();
        let nightly = world.spawn((effect(), NightOnly, visible(false))).id();
        let always = world.spawn((effect(), Visibility::Inherited)).id();
        let shown = |app: &App, e| *app.world().get::<Visibility>(e).unwrap();
        app.world_mut().resource_mut::<Effects>().set_night(true);
        app.update();
        assert_eq!(shown(&app, nightly), Visibility::Inherited);
        app.world_mut().resource_mut::<Effects>().set_night(false);
        app.update();
        assert_eq!(shown(&app, nightly), Visibility::Hidden);
        assert_eq!(shown(&app, always), Visibility::Inherited);
    }

    #[test]
    fn effects_on_things_show_with_them_and_for_the_night_only_at_night() {
        use Visibility::{Hidden, Inherited, Visible};
        assert_eq!(shown_with(Visible, false, false), Inherited);
        assert_eq!(shown_with(Hidden, false, true), Hidden);
        assert_eq!(shown_with(Inherited, true, true), Inherited);
        assert_eq!(shown_with(Visible, true, false), Hidden);
        assert_eq!(shown_with(Hidden, true, true), Hidden);
    }

    fn effects(objects: HashMap<Id, ObjectEffects>) -> Effects {
        Effects {
            vfx: Arc::default(),
            objects,
            operations: HashMap::new(),
            properties: HashMap::new(),
            bones: HashMap::new(),
            decals: DecalCache::default(),
            groups: Vec::new(),
            side: 0,
            queued: Vec::new(),
            shots: HashMap::new(),
            landed: None,
            operated: None,
            aim: 0.65,
            detail: 1.0,
            night: false,
            at_work: Some(2.5),
            rng: 1,
        }
    }

    fn at(tick: u64) -> When {
        When {
            at: tick as f64,
            tick,
            weapon: None,
            landed: None,
        }
    }

    fn gun(effects: tgdata::WeaponEffects, flight: bool) -> Weapon {
        Weapon {
            range: 10.0,
            min_range: 0.0,
            recharge: 1.0,
            area: 0.0,
            max_targets: 0,
            full_within: 0.0,
            edge_damage: 1.0,
            targets: Targets::default(),
            hits: Vec::new(),
            centred_on_user: false,
            animation: None,
            turret: false,
            turret_point: None,
            aimed_by: None,
            spawn_position: [0.0; 3],
            sounds: Vec::new(),
            effects,
            projectile: Some(Id::from("shell")),
            flight: flight.then_some(tgdata::Flight {
                kind: tgdata::FlightKind::Straight,
                speed: 50.0,
                life: 0.0,
            }),
            bombard: false,
            vorpal: false,
            spell: false,
            choice: Default::default(),
            shown: Default::default(),
        }
    }

    #[test]
    fn shots_queue_attacks_by_frequency_and_projectiles_on_landing() {
        let shell = ObjectEffects {
            destroyed: vec![cue("boom")],
            ..ObjectEffects::default()
        };
        let mut effects = effects(HashMap::from([(Id::from("shell"), shell)]));
        let mut weapon_effects = tgdata::WeaponEffects::default();
        weapon_effects.start.push(cue("muzzle"));
        weapon_effects.shot.push(VfxCue {
            every: 2,
            delay: 0.5,
            ..cue("tracer")
        });
        weapon_effects.per_target.push(cue("spark"));
        let me = Target::Unit(0);
        let mut seen = Seen {
            health: 1.0,
            ..Seen::default()
        };
        let shooting = |weapon: &Weapon, effects: &mut Effects, seen: &mut Seen, tick| {
            let now = Now {
                states: Vec::new(),
                health: 1.0,
                shot: Some((tick, 0)),
                weapon: Some(weapon),
                target: Some(Target::Unit(1)),
                finished: false,
                site: false,
                held: Vec::new(),
            };
            effects.witness(me, "tank", seen, now, at(tick));
        };
        // Fired directly, its per-target effects play with the shot.
        let direct = gun(weapon_effects.clone(), false);
        for tick in [1, 2, 3] {
            shooting(&direct, &mut effects, &mut seen, tick);
        }
        let queued = |effects: &Effects| -> Vec<(f64, String, Source)> {
            (effects.queued.iter())
                .map(|(t, p)| (*t, p.cue.vfx.to_string(), p.source))
                .collect()
        };
        let thing = Source::Thing(me);
        let q = |t: f64, v: &str, s| (t, v.to_string(), s);
        assert_eq!(
            queued(&effects),
            [
                q(1.0, "muzzle", thing),
                q(1.5, "tracer", thing),
                q(1.0, "spark", thing),
                q(2.0, "muzzle", thing),
                q(2.0, "spark", thing),
                q(3.0, "muzzle", thing),
                q(3.5, "tracer", thing),
                q(3.0, "spark", thing),
            ]
        );
        // A projectile's land with it, where it landed.
        effects.queued.clear();
        let shelled = gun(weapon_effects, true);
        shooting(&shelled, &mut effects, &mut seen, 4);
        assert_eq!(queued(&effects), [q(4.0, "muzzle", thing)]);
        let landing = Landing {
            tick: 9,
            from: me,
            weapon: 0,
            target: Target::Unit(1),
            at: [5.0, 6.0],
        };
        effects.land(&landing, &shelled, 9.0);
        let landed: Vec<_> = (effects.queued[1..].iter())
            .map(|(_, p)| (p.cue.vfx.as_str(), p.source, p.tick, p.landed))
            .collect();
        assert_eq!(
            landed,
            [
                ("boom", Source::Projectile(me), 9, Some([5.0, 6.0])),
                ("spark", thing, 9, None),
            ]
        );
    }

    #[test]
    fn entering_a_state_or_being_finished_plays_its_effects_once() {
        let depot = ObjectEffects {
            entered: vec![(Id::from("structure_unpacking"), cue("dust"))],
            constructed: vec![cue("lights")],
            destroyed: vec![cue("collapse")],
            ..ObjectEffects::default()
        };
        let mut effects = effects(HashMap::from([(Id::from("depot"), depot)]));
        let mut seen = Seen {
            health: 1.0,
            ..Seen::default()
        };
        let me = Target::Building(0);
        let states = [
            vec!["structure_unpacking"],
            vec!["structure_unpacking"],
            vec![],
            vec![],
            vec![DEAD],
        ];
        for states in states {
            let now = Now {
                finished: states.is_empty(),
                states,
                health: 1.0,
                shot: None,
                weapon: None,
                target: None,
                site: false,
                held: Vec::new(),
            };
            effects.witness(me, "depot", &mut seen, now, at(0));
        }
        let queued: Vec<&str> = (effects.queued.iter())
            .map(|(_, p)| p.cue.vfx.as_str())
            .collect();
        assert_eq!(queued, ["dust", "lights", "collapse"]);
    }

    #[test]
    fn a_property_shows_its_effects_on_what_holds_it_until_it_goes() {
        let mut effects = effects(HashMap::new());
        let radiation = Id::from("radiation");
        (effects.properties).insert(radiation.clone(), vec![cue("sparks")]);
        let me = Target::Unit(0);
        let mut seen = Seen {
            health: 1.0,
            ..Seen::default()
        };
        let mut witness = |effects: &mut Effects, held| {
            let now = Now {
                states: Vec::new(),
                health: 1.0,
                shot: None,
                weapon: None,
                target: None,
                finished: false,
                site: false,
                held,
            };
            effects.witness(me, "tank", &mut seen, now, at(0));
            (effects.queued.iter())
                .map(|(_, p)| {
                    (
                        p.cue.vfx.to_string(),
                        p.cue.stop,
                        p.target,
                        p.holding.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let held = Some(radiation.clone());
        let start = ("sparks".to_string(), false, Some(me), held.clone());
        let stop = ("sparks".to_string(), true, Some(me), held);
        assert_eq!(
            witness(&mut effects, vec![radiation.clone()]),
            std::slice::from_ref(&start)
        );
        assert_eq!(witness(&mut effects, vec![radiation]), [start]);
        // Gone before its effects began, they never do.
        assert_eq!(witness(&mut effects, vec![]), [stop]);
    }

    #[test]
    fn the_strongest_tint_shows_and_one_it_hid_carries_on_after() {
        let tint = |colour, rise, priority| VfxTint {
            colour,
            max: 1.0,
            rise,
            fall: 0.0,
            repeating: false,
            priority,
        };
        let (weak, strong, other) = (Id::from("weak"), Id::from("strong"), Id::from("other"));
        let weak = (&weak, tint([0.0; 3], 2.0, 0), false);
        let strong = (&strong, tint([0.5; 3], 0.0, 1), true);
        let other = (&other, tint([0.25; 3], 0.0, 0), false);
        let mut t = Tinting::default();
        assert_eq!(t.colour(&[weak], [0.0, 50.0]), Some([1.0; 3]));
        // The strong one runs on the wall clock, the weak on game time.
        assert_eq!(t.colour(&[weak, strong], [1.0, 60.0]), Some([0.5; 3]));
        assert_eq!(t.colour(&[weak], [1.5, 70.0]), Some([0.25; 3]));
        assert_eq!(t.colour(&[], [2.0, 80.0]), None);
        assert!(t.since.is_empty());
        // Of equal ones, the first.
        assert_eq!(t.colour(&[other, weak], [3.0, 90.0]), Some([0.25; 3]));
    }

    #[test]
    fn being_created_plays_a_templates_effects_for_it() {
        let wreck = ObjectEffects {
            created: vec![cue("smoke")],
            ..ObjectEffects::default()
        };
        let mut effects = effects(HashMap::from([(Id::from("wreck"), wreck)]));
        let scenery = Source::Scenery(Entity::PLACEHOLDER);
        effects.create(scenery, "wreck", at(0));
        effects.create(scenery, "rock", at(0));
        let queued: Vec<(&str, Source)> = (effects.queued.iter())
            .map(|(_, p)| (p.cue.vfx.as_str(), p.source))
            .collect();
        assert_eq!(queued, [("smoke", scenery)]);
    }

    #[test]
    fn hits_land_on_the_edge_facing_the_attacker() {
        let circle = Shape::Circle { radius: 2.0 };
        let rect = Shape::Rect {
            width: 2.0,
            height: 6.0,
        };
        let north = Vec2::Y;
        assert_eq!(edge(circle, north, Vec2::X), 2.0);
        assert_eq!(edge(Shape::Point, north, Vec2::X), 0.0);
        // Across a target facing north, half its width; along, half its
        // length; corner-on, as far as the nearer side.
        assert!((edge(rect, north, Vec2::X) - 1.0).abs() < 1e-5);
        assert!((edge(rect, north, -Vec2::Y) - 3.0).abs() < 1e-5);
        let diagonal = Vec2::ONE.normalize();
        assert!((edge(rect, north, diagonal) - 2f32.sqrt()).abs() < 1e-5);
    }

    #[test]
    fn hits_land_at_the_attackers_aim_height_within_the_target() {
        let tank = [0.0, 3.0];
        // A tank's hit on a tall building, at two thirds of the tank.
        assert!((hit_height(0.65, Some(tank), [0.0, 12.0]) - 1.95).abs() < 1e-5);
        // A tall ship's on a low jeep, on the jeep's top.
        assert_eq!(hit_height(0.65, Some([0.0, 20.0]), [0.0, 1.5]), 1.5);
        assert!((hit_height(0.5, None, [0.0, 4.0]) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn a_duration_stretches_the_models_animation_to_it() {
        let stretched = VfxCue {
            duration: Some(4.0),
            ..cue("smoke")
        };
        assert_eq!(rate(&vfx(false, None), &stretched, Some(2.0)), Some(0.5));
        assert_eq!(
            rate(&vfx(false, Some(1.0)), &cue("smoke"), Some(2.0)),
            Some(2.0)
        );
        assert_eq!(rate(&vfx(false, None), &cue("smoke"), Some(2.0)), None);
        assert_eq!(rate(&vfx(false, None), &stretched, None), None);
    }
}
