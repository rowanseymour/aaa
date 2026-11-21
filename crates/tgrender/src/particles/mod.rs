//! Particle systems in the game's models: born from their emitters, moved by
//! their modifiers each frame and drawn as squares facing the camera.
//!
//! A system and the objects its emitter and modifiers name are anchors: the
//! node above each one in the model, and where it is below that node. On a
//! rig the anchors hang from its node entities, so they follow its
//! animation; otherwise they hang from the model's root where the model has
//! them.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use nif::{BoolKeys, ControllerKind, EmitterShape, Keys, ModifierKind, Nif, Object};

use crate::models::{Part, controllers};
use crate::space::in_world;

mod draw;
mod play;

pub(crate) use draw::Layers;
pub use draw::{Draws, ParticleMaterial, draw};
pub(crate) use play::spawn;
pub use play::{ParticleMesh, Particles, place_copies, play};

/// A model's particle system, as read from it once for every object that
/// draws the model.
pub(crate) struct System {
    /// Where things are: the system itself first.
    anchors: Vec<Anchor>,
    /// Particles stay where they're born as the model moves.
    world_space: bool,
    /// The most particles it holds at once.
    capacity: usize,
    steps: Vec<Step>,
    look: Look,
    /// How fast its particles age and move against the model's clock.
    clock: f32,
    /// The meshes each particle shows a copy of instead of a square, by
    /// generation: the last for any later one.
    copies: Vec<Arc<Vec<Part>>>,
}

/// How a particle system's squares are drawn.
#[derive(Clone, Copy, Default)]
pub(crate) struct Look {
    /// Its texture's layer of [`draw::TEXTURES`].
    pub layer: u32,
    /// The colour of its squares, linear, before their texture and their
    /// particle's colour.
    pub colour: [f32; 4],
    /// The colour they give off, linear, if lit.
    pub emissive: [f32; 3],
    pub lit: bool,
    pub blend: Blend,
    /// Its squares take each particle's colour, which its lighting may
    /// ignore.
    pub tinted: bool,
}

impl Look {
    /// This look for a system with `steps`: without colour keys among them,
    /// its squares aren't tinted.
    fn coloured<'a>(self, steps: impl IntoIterator<Item = &'a StepKind>) -> Look {
        let colours = steps.into_iter().any(|k| matches!(k, StepKind::Colour(_)));
        Look {
            tinted: self.tinted && colours,
            ..self
        }
    }
}

/// How a square goes over what's behind it.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(crate) enum Blend {
    /// Mixed by its alpha.
    #[default]
    Over,
    /// Added, scaled by its alpha.
    Add,
    /// Covering it, except where its alpha is under `cutoff`.
    Opaque { cutoff: f32 },
}

/// Where one of a model's scene objects is.
#[derive(Clone, Copy)]
pub(crate) struct Anchor {
    /// The nif index of the node above it, or the node itself.
    pub node: usize,
    /// From the node to it.
    pub below: nif::Transform,
    /// From the model's root to it.
    pub model: nif::Transform,
}

/// One of a system's modifiers, run each frame in the model's order.
struct Step {
    kind: StepKind,
    /// Switches it on and off over the system's time, else always as set.
    active: Option<Timed<BoolKeys>>,
    on: bool,
}

/// A controller's keys, with how they're timed.
struct Timed<K> {
    controller: nif::Controller,
    keys: K,
}

impl<K: nif::ObjectType + Clone> Timed<K> {
    /// The keys `controller` plays, timed as it times them.
    fn of(nif: &Nif, controller: &nif::Controller) -> Option<Self> {
        Some(Timed {
            keys: nif.get_as::<K>(controller.data)?.clone(),
            controller: controller.clone(),
        })
    }
}

impl<K> Timed<K> {
    fn time(&self, t: f32) -> f32 {
        self.controller.key_time(t)
    }

    /// Whether its key time wraps round to the start, rather than holding
    /// or turning back, at the end.
    fn loops(&self) -> bool {
        self.controller.cycle() == nif::Cycle::Loop
    }

    /// Whether its key time wraps round between `before` and `t`, however
    /// many cycles apart they are.
    fn wraps(&self, before: f32, t: f32) -> bool {
        let c = &self.controller;
        let span = c.stop - c.start;
        if !self.loops() || span <= 0.0 {
            return false;
        }
        let cycle = |t: f32| ((t * c.frequency + c.phase - c.start) / span).floor();
        cycle(t) != cycle(before)
    }
}

enum StepKind {
    Age,
    Emit(Box<Emitter>),
    Gravity {
        /// The anchor it's measured from, or the system's own.
        anchor: usize,
        axis: Vec3,
        decay: f32,
        strength: f32,
        spherical: bool,
        turbulence: f32,
    },
    Drag {
        anchor: usize,
        axis: Vec3,
        percentage: f32,
        range: f32,
        falloff: f32,
    },
    Move,
    GrowFade(nif::GrowFade),
    Colour(Keys<[f32; 4]>),
    Collide(Vec<Collider>),
    /// Turns each particle about its axis, given or random per particle.
    Spin {
        random: bool,
        axis: Vec3,
        speed: f32,
    },
}

/// Something particles bounce off, or die on.
struct Collider {
    /// The anchor that places it, or the system's own.
    anchor: usize,
    bounce: f32,
    die: bool,
    shape: ColliderShape,
}

enum ColliderShape {
    /// Spanned by its axes, out to its width and height along them.
    Plane {
        x_axis: Vec3,
        y_axis: Vec3,
        width: f32,
        height: f32,
    },
    Sphere {
        radius: f32,
    },
}

struct Emitter {
    settings: nif::Emitter,
    shape: Shape,
    /// The birth rate and when it's on.
    keys: Option<Timed<nif::EmitterKeys>>,
    radius: Option<Timed<Keys<f32>>>,
    speed: Option<Timed<Keys<f32>>>,
    /// Births count from the start of each cycle of its keys, as in the
    /// older format, rather than from when it came on.
    cycles: bool,
    /// Every particle goes when its keys wrap round.
    reset: bool,
}

enum Shape {
    Box {
        anchor: usize,
        size: Vec3,
    },
    Cylinder {
        anchor: usize,
        radius: f32,
        height: f32,
    },
    Sphere {
        anchor: usize,
        radius: f32,
    },
    Mesh {
        meshes: Vec<EmitterMesh>,
        velocity: u32,
        emission: u32,
        axis: Vec3,
    },
}

/// A mesh particles are born on, in its own space.
struct EmitterMesh {
    anchor: usize,
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    triangles: Vec<[u16; 3]>,
}

/// The scene objects of a model, by nif index, as [`crate::rig::Nodes`]
/// finds them.
#[derive(Default)]
pub(crate) struct Frames(pub HashMap<usize, Anchor>);

impl Frames {
    /// Adds where the object `r` is to `anchors`, returning its index
    /// there; `None` if it's not one of the model's scene objects.
    fn add_anchor(&self, anchors: &mut Vec<Anchor>, r: nif::Ref) -> Option<usize> {
        anchors.push(*self.0.get(&r?)?);
        Some(anchors.len() - 1)
    }
}

impl System {
    /// The system at nif index `at`, drawn in `material`, or `None` if it
    /// can't be placed.
    pub fn read(nif: &Nif, frames: &Frames, at: usize, look: Look) -> Option<System> {
        let system = nif.get_as::<nif::ParticleSystem>(Some(at))?;
        let mut anchors = vec![*frames.0.get(&at)?];
        let mut anchor = |r| frames.add_anchor(&mut anchors, r);
        let chain: Vec<&nif::Controller> = controllers(nif, system.shape.av.controller).collect();
        let timed = |kind: ControllerKind, name: &str| {
            chain
                .iter()
                .find(|c| c.kind == kind && c.modifier == name)
                .copied()
        };
        let mut steps = Vec::new();
        for m in system
            .modifiers
            .iter()
            .filter_map(|&r| nif.get_as::<nif::Modifier>(r))
        {
            let kind = match &m.kind {
                ModifierKind::AgeDeath { .. } => StepKind::Age,
                ModifierKind::Position => StepKind::Move,
                ModifierKind::ColliderManager { collider } => {
                    let mut colliders = Vec::new();
                    let mut next = *collider;
                    while let Some(c) = nif.get_as::<nif::Collider>(next) {
                        if colliders.len() >= 16 {
                            break;
                        }
                        next = c.next;
                        let shape = match &c.shape {
                            nif::ColliderShape::Plane {
                                width,
                                height,
                                x_axis,
                                y_axis,
                            } => ColliderShape::Plane {
                                x_axis: in_world(*x_axis),
                                y_axis: in_world(*y_axis),
                                width: *width,
                                height: *height,
                            },
                            nif::ColliderShape::Sphere { radius } => {
                                ColliderShape::Sphere { radius: *radius }
                            }
                        };
                        colliders.push(Collider {
                            anchor: anchor(c.object).unwrap_or(0),
                            bounce: c.bounce,
                            die: c.die_on_collide,
                            shape,
                        });
                    }
                    if colliders.is_empty() {
                        continue;
                    }
                    StepKind::Collide(colliders)
                }
                ModifierKind::GrowFade(g) => StepKind::GrowFade(g.clone()),
                ModifierKind::Color { data } => match nif.get_as::<Keys<[f32; 4]>>(*data) {
                    Some(keys) => StepKind::Colour(keys.clone()),
                    None => continue,
                },
                ModifierKind::Gravity(g) => StepKind::Gravity {
                    anchor: anchor(g.object).unwrap_or(0),
                    axis: in_world(g.axis).normalize_or_zero(),
                    decay: g.decay,
                    strength: g.strength,
                    spherical: g.force == 1,
                    turbulence: g.turbulence * g.turbulence_scale,
                },
                // Drag needs an object to measure its range from.
                ModifierKind::Drag(d) => match anchor(d.object) {
                    Some(a) => StepKind::Drag {
                        anchor: a,
                        axis: in_world(d.axis).normalize_or_zero(),
                        percentage: d.percentage,
                        range: d.range,
                        falloff: d.falloff,
                    },
                    None => continue,
                },
                ModifierKind::Emitter(e) => {
                    let shape = match &e.shape {
                        EmitterShape::Box { object, size } => Shape::Box {
                            anchor: anchor(*object).unwrap_or(0),
                            size: Vec3::from(*size),
                        },
                        EmitterShape::Cylinder {
                            object,
                            radius,
                            height,
                        } => Shape::Cylinder {
                            anchor: anchor(*object).unwrap_or(0),
                            radius: *radius,
                            height: *height,
                        },
                        EmitterShape::Sphere { object, radius } => Shape::Sphere {
                            anchor: anchor(*object).unwrap_or(0),
                            radius: *radius,
                        },
                        EmitterShape::Mesh {
                            meshes,
                            velocity,
                            emission,
                            axis,
                        } => {
                            let meshes = meshes
                                .iter()
                                .filter_map(|&r| {
                                    let shape = nif.get_as::<nif::Shape>(r)?;
                                    let g = nif.get_as::<nif::Geometry>(shape.data)?;
                                    Some(EmitterMesh {
                                        anchor: anchor(r)?,
                                        positions: g
                                            .positions
                                            .iter()
                                            .map(|&p| in_world(p))
                                            .collect(),
                                        normals: g.normals.iter().map(|&n| in_world(n)).collect(),
                                        triangles: g.triangles.clone(),
                                    })
                                })
                                .filter(|m| !m.positions.is_empty())
                                .collect::<Vec<_>>();
                            if meshes.is_empty() {
                                continue;
                            }
                            Shape::Mesh {
                                meshes,
                                velocity: *velocity,
                                emission: *emission,
                                axis: in_world(*axis),
                            }
                        }
                    };
                    let keyed = |kind| timed(kind, &m.name);
                    StepKind::Emit(Box::new(Emitter {
                        settings: e.clone(),
                        shape,
                        keys: keyed(ControllerKind::Emitter).and_then(|c| Timed::of(nif, c)),
                        radius: keyed(ControllerKind::EmitterInitialRadius)
                            .and_then(|c| Timed::of(nif, c)),
                        speed: keyed(ControllerKind::EmitterSpeed).and_then(|c| Timed::of(nif, c)),
                        cycles: false,
                        reset: false,
                    }))
                }
                _ => continue,
            };
            let active =
                timed(ControllerKind::ModifierActive, &m.name).and_then(|c| Timed::of(nif, c));
            steps.push(Step {
                kind,
                active,
                on: m.active,
            });
        }
        let capacity = nif
            .get_as::<nif::ParticleData>(system.shape.data)
            .map_or(0, |d| d.capacity);
        let look = look.coloured(steps.iter().map(|s| &s.kind));
        steps
            .iter()
            .any(|s| matches!(s.kind, StepKind::Emit(_)))
            .then_some(System {
                anchors,
                world_space: system.world_space,
                capacity,
                steps,
                look,
                clock: 1.0,
                copies: Vec::new(),
            })
    }

    /// The older-format system `placed` finds, drawn in `look`, or as
    /// `copies` of meshes for mesh particles; `None` if it can't be placed.
    pub fn read_old(
        nif: &Nif,
        frames: &Frames,
        placed: &nif::Placed,
        look: Look,
        copies: Vec<Arc<Vec<Part>>>,
    ) -> Option<System> {
        let (&at, parents) = placed.path.split_last()?;
        let old = nif.get_as::<nif::OldParticles>(Some(at))?;
        let c = old_controller(nif, old)?;
        let data = nif.get_as::<nif::OldParticleData>(old.shape.data)?;
        // It sits at its parent node, without its own transform.
        let home = *frames.0.get(parents.last()?)?;
        let mut anchors = vec![home];
        let mut anchor = |r| frames.add_anchor(&mut anchors, r);
        let capacity = data.geometry.positions.len();
        let t = &c.timing;
        let rate = if c.use_birth_rate {
            c.birth_rate
        } else if t.stop - t.start > c.life_span {
            capacity as f32 / c.life_span
        } else {
            capacity as f32 / (emit_stop(c) - c.emit_start)
        };
        let mut timing = t.clone();
        // The system's clock already runs at its frequency.
        timing.frequency = 1.0;
        if c.reset {
            timing.set_cycle(nif::Cycle::Loop);
        }
        let active = match t.flags & 1 {
            0 => old_switching(t.start, t.stop, c.emit_start, emit_stop(c)),
            _ => vec![(t.start, false)],
        };
        let emitter = Emitter {
            settings: nif::Emitter {
                speed: c.speed,
                speed_variation: c.speed_variation,
                declination: c.declination,
                declination_variation: c.declination_variation,
                planar_angle: c.planar_angle,
                planar_angle_variation: c.planar_angle_variation,
                color: c.color,
                radius: c.size,
                life_span: c.life_span,
                life_span_variation: c.life_span_variation,
                shape: EmitterShape::Box {
                    object: c.emitter,
                    size: c.emitter_size,
                },
            },
            shape: Shape::Box {
                anchor: anchor(c.emitter).unwrap_or(0),
                size: Vec3::from(c.emitter_size),
            },
            keys: Some(Timed {
                controller: timing,
                keys: nif::EmitterKeys {
                    birth_rate: Keys {
                        interpolation: nif::Interpolation::Linear,
                        keys: vec![nif::Key {
                            time: t.start,
                            value: if rate.is_finite() { rate } else { 0.0 },
                            tangents: None,
                        }],
                    },
                    active,
                },
            }),
            radius: None,
            speed: None,
            cycles: true,
            reset: c.reset,
        };
        let mut kinds = vec![StepKind::Age, StepKind::Emit(Box::new(emitter))];
        let mut gravity = Vec::new();
        let own = &old.shape.av.transform;
        for m in old_modifiers(nif, c) {
            match &m.kind {
                nif::OldModifierKind::GrowFade { grow, fade } => {
                    kinds.push(StepKind::GrowFade(nif::GrowFade {
                        grow: *grow,
                        grow_generation: 0,
                        fade: *fade,
                        fade_generation: c.spawn_generations,
                    }))
                }
                nif::OldModifierKind::Color { data } => {
                    if let Some(keys) = nif.get_as::<Keys<[f32; 4]>>(*data) {
                        kinds.push(StepKind::Colour(keys.clone()));
                    }
                }
                // Only mesh copies show the turn, and only of particles
                // with rotations.
                nif::OldModifierKind::Rotation {
                    random_axis,
                    axis,
                    speed,
                } if !data.rotations.is_empty() && !copies.is_empty() => {
                    kinds.push(StepKind::Spin {
                        random: *random_axis,
                        axis: in_world(*axis).normalize_or_zero(),
                        speed: *speed,
                    })
                }
                nif::OldModifierKind::Gravity {
                    decay,
                    strength,
                    force,
                    position,
                    direction,
                } => {
                    // Placed by the particle object's own transform, under
                    // its parent, with its position not turned.
                    let place = nif::Transform {
                        translation: std::array::from_fn(|i| position[i] + own.translation[i]),
                        ..*own
                    };
                    anchors.push(Anchor {
                        node: home.node,
                        below: home.below.then(&place),
                        model: home.model.then(&place),
                    });
                    gravity.push(StepKind::Gravity {
                        anchor: anchors.len() - 1,
                        axis: in_world(*direction).normalize_or_zero(),
                        decay: *decay,
                        strength: *strength,
                        spherical: *force == 1,
                        turbulence: 0.0,
                    });
                }
                _ => {}
            }
        }
        kinds.extend(gravity);
        kinds.push(StepKind::Move);
        let look = look.coloured(&kinds);
        Some(System {
            anchors,
            world_space: false,
            capacity,
            steps: kinds
                .into_iter()
                .map(|kind| Step {
                    kind,
                    active: None,
                    on: true,
                })
                .collect(),
            look,
            clock: t.frequency,
            copies,
        })
    }
}

/// The controller playing an older-format particle object, found down its
/// chain whether or not it's marked active.
pub(crate) fn old_controller<'a>(
    nif: &'a Nif,
    old: &nif::OldParticles,
) -> Option<&'a nif::OldParticleController> {
    let mut next = old.shape.av.controller;
    for _ in 0..16 {
        if let Some(Object::OldParticleController(c)) = nif.get(next) {
            return Some(c);
        }
        next = nif.get_as::<nif::Controller>(next)?.next;
    }
    None
}

/// An older-format controller's modifiers, in the order of its chain.
pub(crate) fn old_modifiers<'a>(
    nif: &'a Nif,
    c: &nif::OldParticleController,
) -> impl Iterator<Item = &'a nif::OldModifier> {
    let mut next = c.modifier;
    std::iter::from_fn(move || {
        let m = nif.get_as::<nif::OldModifier>(next)?;
        next = m.next;
        Some(m)
    })
    .take(16)
}

/// An older-format emitter's stop time, kept apart from its start.
fn emit_stop(c: &nif::OldParticleController) -> f32 {
    match c.emit_stop == c.emit_start {
        true => c.emit_start + 0.00001,
        false => c.emit_stop,
    }
}

/// When an older-format emitter is on, as keys over its controller's
/// `start` to `stop`, from its emit start and stop times.
fn old_switching(start: f32, stop: f32, emit_start: f32, emit_stop: f32) -> BoolKeys {
    // A window ending before the keys or before it opens is never on.
    if emit_start >= stop || emit_stop <= start.max(emit_start) {
        return vec![(start, false)];
    }
    let mut keys = match emit_start > start {
        true => vec![(start, false), (emit_start, true)],
        false => vec![(start, true)],
    };
    if emit_stop < stop {
        keys.push((emit_stop, false));
    }
    keys.push((stop, false));
    keys
}
