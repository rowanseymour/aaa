//! Starting an effect: what of its command fits, where it goes and for
//! how long, as a model or a picture on the ground.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::prelude::*;
use tgdata::{Decal, Id, Shape, Targets, Vfx, VfxCue};
use tgrender::camera::RtsCamera;
use tgrender::models::{Footprint, Rate};
use tgrender::space::map_point;
use tgrender::{MapAssets, MapObject};
use tgsim::{State, Target, World};

use crate::ui::visible;
use crate::views::{BuildingView, UnitView};
use crate::{Drawn, Loaded, Screen, Sim};

use super::follow::{Fading, fade_in, opacity};
use super::notice::{RAISE, fighter};
use super::place::{Place, upright};
use super::{Effect, Effects, Follow, NightOnly, Source, Stopped, Subject, Tied, tie};

/// On an effect that sinks a building's view, the view, for as long as the
/// effect lasts.
#[derive(Component)]
pub struct Lowers(pub Entity);

/// Whether an effect plays at the effects detail setting `detail`, given
/// `roll`, a number in `0..1`: critical ones always do.
pub(super) fn detail_allows(critical: bool, detail: f32, roll: f32) -> bool {
    critical || roll < detail
}

/// Whether a command for the terrain group `filter` plays on ground of
/// `group`.
pub(super) fn terrain_allows(filter: Option<&Id>, group: Option<&Id>) -> bool {
    match filter {
        None => true,
        Some(f) => group.is_some_and(|g| g.as_str().eq_ignore_ascii_case(f.as_str())),
    }
}

/// Seconds an effect lasts, or `None` for one that lasts until stopped: a
/// duration of its command's or its own, else its model's animation once
/// through, which may be no time at all.
pub(super) fn lifetime(vfx: &Vfx, cue: &VfxCue, animation: Option<f32>) -> Option<f32> {
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
pub(super) fn on_screen(camera: Option<(&Camera, &GlobalTransform)>, at: Vec3) -> bool {
    let Some(ndc) = camera.and_then(|(c, g)| c.world_to_ndc(g, at)) else {
        return true;
    };
    ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z)
}

/// A thing's view as effects find it.
pub(super) struct Placed<'a> {
    /// What an effect on it follows.
    pub(super) anchor: Entity,
    /// Where it is.
    pub(super) root: Transform,
    pub(super) thing: &'a tgck::map::Thing,
    /// The model it's drawn with: while it packs or unpacks, its packing
    /// model, unpacking if this is true.
    pub(super) packing: Option<bool>,
    /// The way it faces across the ground, a unit vector.
    pub(super) facing: Vec2,
    /// What it covers of the ground.
    pub(super) footprint: Shape,
    /// It was alive when the event happened.
    pub(super) living: bool,
    /// What it's armed with, if it fights.
    pub(super) fighter: Option<&'a tgdata::Fighter>,
}

/// Plays the queued commands that are due.
#[allow(clippy::too_many_arguments)]
pub(super) fn play(
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
    let due = crate::sound::take_due(&mut effects.queued, now);
    if due.is_empty() {
        return;
    }
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
pub(super) fn rect(size: Option<&Footprint>) -> Shape {
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
pub(super) fn rate(vfx: &Vfx, cue: &VfxCue, length: Option<f32>) -> Option<f32> {
    let duration = cue.duration.or(vfx.duration)?;
    let length = length.filter(|l| *l > 0.0)?;
    (duration > 0.0).then_some(length / duration)
}

/// How far out from its centre a footprint's edge is, going in `dir` from
/// it, for one facing `facing`; both unit vectors across the ground.
pub(super) fn edge(footprint: Shape, facing: Vec2, dir: Vec2) -> f32 {
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
pub(super) fn hit_height(ratio: f32, attacker: Option<[f32; 2]>, target: [f32; 2]) -> f32 {
    let up = |[bottom, top]: [f32; 2]| bottom + ratio * (top - bottom);
    let [bottom, top] = target;
    attacker
        .map_or(up(target), up)
        .clamp(bottom, top.max(bottom))
}

/// Whether a command naming `targets` plays when `by` sets it off on `on`:
/// any does that names no flags.
pub(super) fn fits(sim: &World, targets: &Targets, by: Option<Target>, on: Option<Target>) -> bool {
    match (by, on) {
        _ if targets.required.is_empty() && targets.excluded.is_empty() => true,
        (Some(by), Some(on)) => sim.matches(by, targets, on),
        _ => false,
    }
}
