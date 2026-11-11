//! Plays the visual effects the game data gives objects as things happen to
//! them: dust while buildings pack and unpack, flashes and hit puffs as
//! they fire, explosions where shells land, and smoke, fire and debris as
//! they're hurt. Which of an object's effect commands an event runs, where
//! the effect goes and how long it lasts are the data's; this notices the
//! events in the sim and plays the effects drawn by a model.

use std::collections::HashMap;
use std::f32::consts::TAU;

use aaa_data::{
    GameData, HealthCue, HealthTrigger, Id, ObjectEffects, Shape, Vfx, VfxCue, VfxTint, Weapon,
};
use aaa_map::{Ground, Map};
use aaa_render::camera::RtsCamera;
use aaa_render::models::{Footprint, Models, Rate};
use aaa_render::terrain::{map_point, world};
use aaa_render::{MapAssets, MapObject};
use aaa_sim::{Landing, State, Target, World};
use bevy::prelude::*;
use tgck::map::CELL_SIZE;

use crate::{BuildingView, Drawn, Loaded, Screen, Sim, UnitView};

#[derive(Resource)]
pub struct Effects {
    vfx: HashMap<Id, Vfx>,
    objects: HashMap<Id, ObjectEffects>,
    /// The node names of the model points commands name, by id.
    bones: HashMap<Id, String>,
    /// Each map tile's terrain group, row-major, `side` tiles a row.
    groups: Vec<Option<Id>>,
    side: usize,
    /// Commands waiting to play, with when.
    queued: Vec<(f64, Play)>,
    /// Shots each weapon has fired, by who fired it and the weapon's index.
    shots: HashMap<(Target, usize), u32>,
    /// The tick of the last projectile landing seen.
    landed: Option<u64>,
    /// How far up an attacker a hit on its target lands, as a share of
    /// the attacker's height.
    aim: f32,
    /// The effects detail setting, from 0 for none to 1 for all, which
    /// effects that aren't critical play by.
    detail: f32,
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
    /// For a projectile, where it landed.
    landed: Option<[f32; 2]>,
    /// For an attack, the index of the weapon among its attacker's.
    weapon: Option<usize>,
}

/// What set off an effect command.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    Thing(Target),
    /// The projectile of a shot fired by this, which has reached its
    /// target.
    Projectile(Target),
}

/// What a view's thing was last seen doing, to notice the events of.
#[derive(Component)]
struct Seen {
    shot: Option<(u64, usize)>,
    /// A share of its maximum.
    health: f32,
    states: Vec<&'static str>,
}

/// One of the effects playing.
#[derive(Component)]
struct Effect {
    vfx: Id,
    /// Whose command created it, for a stop command to find it by.
    by: Option<Target>,
    /// When it ends, unless it lasts until stopped.
    ends: Option<f64>,
    /// It goes once its spot is off screen.
    discard: bool,
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

    pub fn new(game: &mut GameData, map: &Map) -> Effects {
        let vfx = game.vfx();
        let objects = game.object_effects();
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
        debug!(
            "{} effects shown by {} object templates, at {} named points",
            vfx.len(),
            objects.len(),
            bones.len()
        );
        Effects {
            vfx,
            objects,
            bones,
            groups,
            side: terrain.side,
            queued: Vec::new(),
            shots: HashMap::new(),
            landed: None,
            aim: game.aim_heights().straight,
            detail: 1.0,
            rng: 0x9e37_79b9_7f4a_7c15,
        }
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
        for cue in cues {
            let play = Play {
                cue: cue.clone(),
                source,
                target,
                tick: when.tick,
                landed: None,
                weapon: when.weapon,
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
        };
        let target = Some(landing.target);
        let landed = (weapon.projectile.as_ref())
            .and_then(|p| self.objects.get(p))
            .map(|o| o.destroyed.clone())
            .unwrap_or_default();
        let from = self.queued.len();
        self.queue(when, &landed, Source::Projectile(landing.from), target);
        for (_, p) in &mut self.queued[from..] {
            p.landed = Some(landing.at);
        }
        let per_target = weapon.effects.per_target.clone();
        self.queue(when, &per_target, Source::Thing(landing.from), target);
    }

    /// Queues the commands of what `me`, of `template`, did since it was
    /// `seen`, and notes what it's doing `now`.
    fn witness(&mut self, me: Target, template: &str, seen: &mut Seen, now: Now, at: When) {
        let objects = self.objects.get(&Id::from(template)).cloned();
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
                let cues = by_state(&o.left, state);
                self.queue(at, &cues, Source::Thing(me), None);
            }
            if now.health != seen.health {
                let cues = crossed(&o.health, seen.health, now.health);
                self.queue(at, &cues, Source::Thing(me), None);
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
        seen.shot = now.shot;
        seen.health = now.health;
        seen.states = now.states;
    }
}

/// When something happened: in seconds played and the sim's ticks.
#[derive(Clone, Copy)]
struct When {
    at: f64,
    tick: u64,
    /// For an attack, the index of its weapon.
    weapon: Option<usize>,
}

/// What a thing is doing now, to compare with what it was [`Seen`] doing.
struct Now<'a> {
    states: Vec<&'static str>,
    health: f32,
    shot: Option<(u64, usize)>,
    weapon: Option<&'a Weapon>,
    target: Option<Target>,
}

const DEAD: &str = "dead";

/// The states a building is in, as effect commands name them.
fn building_states(b: &aaa_sim::Building) -> Vec<&'static str> {
    let mut states = Vec::new();
    if b.packing.is_some() {
        states.push("structure_packing");
    }
    if b.unpacking.is_some() {
        states.push("structure_unpacking");
    }
    if matches!(b.state, State::Destroyed(_)) {
        states.push(DEAD);
    }
    states
}

fn unit_states(u: &aaa_sim::Unit) -> Vec<&'static str> {
    // Not one that left the map by unpacking, which is no death.
    match u.died {
        None => Vec::new(),
        Some(_) => vec![DEAD],
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
            (notice, play, follow)
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
/// seen.
fn notice(
    mut commands: Commands,
    sim: Res<Sim>,
    time: Res<Time>,
    mut effects: ResMut<Effects>,
    mut sounds: ResMut<crate::sound::Sounds>,
    mut units: Query<(Entity, &UnitView, Option<&mut Seen>)>,
    mut buildings: Query<(Entity, &BuildingView, Option<&mut Seen>), Without<UnitView>>,
) {
    let sim = &sim.0;
    let at = When {
        at: time.elapsed_secs_f64(),
        tick: sim.ticks,
        weapon: None,
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
    for (e, view, seen) in &mut units {
        let unit = &sim.units[view.unit];
        let fighter = unit.fighter.as_ref();
        let now = Now {
            states: unit_states(unit),
            health: share(unit.health, fighter.map_or(0.0, |f| f.health)),
            shot: unit.shot,
            weapon: unit.shot.and_then(|(_, w)| fighter?.weapons.get(w)),
            target: unit.target,
        };
        let me = Target::Unit(view.unit);
        see(
            &mut commands,
            &mut effects,
            e,
            seen,
            me,
            &unit.template,
            now,
            at,
        );
    }
    for (e, view, seen) in &mut buildings {
        let b = &sim.buildings[view.building];
        let now = Now {
            states: building_states(b),
            health: share(b.health, b.fighter.health),
            shot: b.shot,
            weapon: b.shot.and_then(|(_, w)| b.fighter.weapons.get(w)),
            target: b.target,
        };
        let me = Target::Building(view.building);
        see(
            &mut commands,
            &mut effects,
            e,
            seen,
            me,
            &b.template,
            now,
            at,
        );
    }
}

/// Whose a target is.
fn owner(sim: &World, t: Target) -> u32 {
    match t {
        Target::Unit(u) => sim.regiments[sim.units[u].regiment].owner,
        Target::Building(b) => sim.buildings[b].owner,
    }
}

/// What a target is, whether it's living or not.
fn fighter(sim: &World, t: Target) -> Option<&aaa_data::Fighter> {
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
                states: Vec::new(),
            };
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
    fighter: Option<&'a aaa_data::Fighter>,
}

/// Plays the queued commands that are due.
#[allow(clippy::too_many_arguments)]
fn play(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<Sim>,
    mut effects: ResMut<Effects>,
    mut loaded: ResMut<Loaded>,
    mut drawn: ResMut<Drawn>,
    mut assets: MapAssets,
    units: Query<(Entity, &UnitView, Option<&Footprint>)>,
    buildings: Query<(Entity, &BuildingView, Option<&Footprint>)>,
    things: Query<(&Transform, &MapObject)>,
    playing: Query<(Entity, &Effect)>,
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
    let placed = |t: Target, tick: u64| {
        let &(anchor, root, packing, footprint) = views.get(&t)?;
        let (transform, object) = things.get(root).ok()?;
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
    let Loaded { game, map } = &mut *loaded;
    for (_, p) in due {
        let cue = &p.cue;
        let (by, fired_by) = match p.source {
            Source::Thing(t) => (Some(t), t),
            Source::Projectile(t) => (None, t),
        };
        if cue.stop {
            for (e, effect) in &playing {
                if effect.by == by && by.is_some() && effect.vfx == cue.vfx {
                    commands.entity(e).despawn();
                }
            }
            continue;
        }
        let Some(vfx) = effects.vfx.get(&cue.vfx).cloned() else {
            continue;
        };
        let Some(path) = vfx.model.as_deref().filter(|_| vfx.category == "VFXMODEL") else {
            continue;
        };
        if !cue.targets.required.is_empty() || !cue.targets.excluded.is_empty() {
            let Some(t) = p.target else {
                continue;
            };
            if !sim.matches(fired_by, &cue.targets, t) {
                continue;
            }
        }
        let place = Place {
            placed: &|t| placed(t, p.tick),
            ground: &map.ground,
            bones: &effects.bones,
            aim: effects.aim,
        };
        let spotted = place.spot(&p, &mut drawn.0, game);
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
        let length = drawn.0.effect_timing(game, path).map(|t| t.length);
        let lasts = lifetime(&vfx, cue, length);
        let discard = lasts.is_some() && !vfx.never_discard;
        if discard && !on_screen(camera, transform.translation) {
            continue;
        }
        let fogged = fog.at(map_point(transform.translation)) != aaa_sim::Seen::Visible;
        if lasts.is_some() && fogged {
            continue;
        }
        let turn = match vfx.random_orientation {
            true => Quat::from_rotation_y(effects.random() * TAU),
            false => Quat::IDENTITY,
        };
        let unturned = transform.rotation;
        transform.rotation = turn * unturned;
        let mut models = assets.models();
        let Some(e) = drawn.0.spawn_effect(
            &mut commands,
            game,
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
                ends: lasts.map(|s| now + f64::from(s)),
                discard,
            },
            DespawnOnExit(Screen::Match),
        ));
        if let Some(rate) = rate(&vfx, cue, length) {
            entity.insert(Rate(rate));
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
    placed: &'b dyn Fn(Target) -> Option<Placed<'a>>,
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
        game: &mut GameData,
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
            let mut t = Transform::from_translation(world(x, y, self.ground.height_at(x, y)));
            t.translation += offset;
            t.rotation = turn;
            t
        };
        let (attacker, own) = match *source {
            Source::Thing(t) => (Some(t), Some(t)),
            Source::Projectile(t) => (Some(t), None),
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
            _ => {}
        }
        let subject = match at {
            "TARGET_THING" | "TARGET_GROUND" | "TARGET_HIT_POSITION" => (*target)?,
            "SOURCE_ABILITY_POSITION" => attacker?,
            _ => own?,
        };
        let thing = (self.placed)(subject)?;
        let here = map_point(thing.root.translation);
        let extent = |models: &mut Models, game: &mut GameData, p: &Placed| {
            let path = models.model_path(p.thing, p.packing)?.to_string();
            let [bottom, top] = models.extent(game, &path)?;
            let scale = p.root.scale.y;
            Some([bottom * scale, top * scale])
        };
        match at {
            "SOURCE_GROUND" | "TARGET_GROUND" => Some((on_ground(here), None)),
            "TARGET_HIT_POSITION" => {
                if !thing.living {
                    return None;
                }
                let target_extent = extent(models, game, &thing)?;
                let by = attacker.and_then(self.placed);
                let across = match &by {
                    Some(a) => {
                        let dir = -a.facing;
                        Vec2::from(here) + dir * edge(thing.footprint, thing.facing, dir)
                    }
                    None => Vec2::from(here),
                };
                let height = match by {
                    Some(a) => hit_height(self.aim, extent(models, game, &a), target_extent),
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
                if let Some(point) =
                    turret.and_then(|id| self.point(models, game, &thing, id.as_str()))
                {
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
                    Some(id) => match (self.point(models, game, &thing, id), cue.attached) {
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
    fn point(
        &self,
        models: &mut Models,
        game: &mut GameData,
        thing: &Placed,
        id: &str,
    ) -> Option<Transform> {
        let name = self.bones.get(&Id::from(id)).map_or(id, |n| n);
        let path = models.model_path(thing.thing, thing.packing)?.to_string();
        models.point(game, &path, name)
    }
}

/// The way a command's orientation, in the game's axes, points an effect:
/// the default, the -y its models face along, leaves it as it is.
fn facing([x, y, z]: [f32; 3]) -> Vec3 {
    // The game's -y is Bevy's +z.
    world(x, y, z).try_normalize().unwrap_or(Vec3::Z)
}

/// Moves attached effects with what they're on, and ends those whose time
/// is up, whose thing has gone, or that went off screen.
fn follow(
    mut commands: Commands,
    time: Res<Time>,
    mut effects: Query<(Entity, &Effect, Option<&Follow>, &mut Transform)>,
    anchors: Query<(&Transform, &Visibility, Has<BuildingView>), Without<Effect>>,
    cameras: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
) {
    let now = time.elapsed_secs_f64();
    let camera = cameras.iter().find(|(c, _)| c.is_active);
    for (e, effect, follow, mut transform) in &mut effects {
        if effect.ends.is_some_and(|t| t <= now) {
            commands.entity(e).despawn();
            continue;
        }
        if let Some(f) = follow {
            // A building's hidden once it's gone, though its view stays.
            let anchor = anchors
                .get(f.anchor)
                .ok()
                .filter(|(_, v, building)| !(*building && **v == Visibility::Hidden));
            let Some((root, _, _)) = anchor else {
                commands.entity(e).despawn();
                continue;
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

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_data::Targets;

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

    fn effects(objects: HashMap<Id, ObjectEffects>) -> Effects {
        Effects {
            vfx: HashMap::new(),
            objects,
            bones: HashMap::new(),
            groups: Vec::new(),
            side: 0,
            queued: Vec::new(),
            shots: HashMap::new(),
            landed: None,
            aim: 0.65,
            detail: 1.0,
            rng: 1,
        }
    }

    fn at(tick: u64) -> When {
        When {
            at: tick as f64,
            tick,
            weapon: None,
        }
    }

    fn gun(effects: aaa_data::WeaponEffects, flight: bool) -> Weapon {
        Weapon {
            range: 10.0,
            min_range: 0.0,
            recharge: 1.0,
            area: 0.0,
            max_targets: 0,
            full_within: 0.0,
            edge_damage: 1.0,
            targets: Targets::default(),
            damage: Vec::new(),
            animation: None,
            turret: false,
            turret_point: None,
            aimed_by: None,
            spawn_position: [0.0; 3],
            sounds: Vec::new(),
            effects,
            projectile: Some(Id::from("shell")),
            flight: flight.then_some(aaa_data::Flight {
                kind: aaa_data::FlightKind::Straight,
                speed: 50.0,
                life: 0.0,
            }),
            bombard: false,
        }
    }

    #[test]
    fn shots_queue_attacks_by_frequency_and_projectiles_on_landing() {
        let shell = ObjectEffects {
            destroyed: vec![cue("boom")],
            ..ObjectEffects::default()
        };
        let mut effects = effects(HashMap::from([(Id::from("shell"), shell)]));
        let mut weapon_effects = aaa_data::WeaponEffects::default();
        weapon_effects.start.push(cue("muzzle"));
        weapon_effects.shot.push(VfxCue {
            every: 2,
            delay: 0.5,
            ..cue("tracer")
        });
        weapon_effects.per_target.push(cue("spark"));
        let me = Target::Unit(0);
        let mut seen = Seen {
            shot: None,
            health: 1.0,
            states: Vec::new(),
        };
        let shooting = |weapon: &Weapon, effects: &mut Effects, seen: &mut Seen, tick| {
            let now = Now {
                states: Vec::new(),
                health: 1.0,
                shot: Some((tick, 0)),
                weapon: Some(weapon),
                target: Some(Target::Unit(1)),
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
    fn entering_a_state_plays_its_effects_once() {
        let depot = ObjectEffects {
            entered: vec![(Id::from("structure_unpacking"), cue("dust"))],
            destroyed: vec![cue("collapse")],
            ..ObjectEffects::default()
        };
        let mut effects = effects(HashMap::from([(Id::from("depot"), depot)]));
        let mut seen = Seen {
            shot: None,
            health: 1.0,
            states: Vec::new(),
        };
        let me = Target::Building(0);
        let states = [
            vec!["structure_unpacking"],
            vec!["structure_unpacking"],
            vec![],
            vec![DEAD],
        ];
        for states in states {
            let now = Now {
                states,
                health: 1.0,
                shot: None,
                weapon: None,
                target: None,
            };
            effects.witness(me, "depot", &mut seen, now, at(0));
        }
        let queued: Vec<&str> = (effects.queued.iter())
            .map(|(_, p)| p.cue.vfx.as_str())
            .collect();
        assert_eq!(queued, ["dust", "collapse"]);
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
