//! Particle systems in the game's models: born from their emitters, moved by
//! their modifiers each frame and drawn as squares facing the camera.
//!
//! A system and the objects its emitter and modifiers name are anchors: the
//! node above each one in the model, and where it is below that node. On a
//! rig the anchors hang from its node entities, so they follow its
//! animation; otherwise they hang from the model's root where the model has
//! them.

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::ops::Range;
use std::sync::Arc;

use bevy::asset::{AssetMut, RenderAssetUsages, uuid_handle};
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, TextureFormat};
use bevy::shader::ShaderRef;
use nif::{BoolKeys, ControllerKind, EmitterShape, Keys, ModifierKind, Nif, Object};

use crate::loader::TextureSource;
use crate::models::{Animated, MaterialStores, Part, bevy_transform, controllers};
use crate::terrain::{self, world};
use crate::{blend, textures};

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
    /// Its texture's layer of [`TEXTURES`].
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

impl System {
    /// The system at nif index `at`, drawn in `material`, or `None` if it
    /// can't be placed.
    pub fn read(nif: &Nif, frames: &Frames, at: usize, look: Look) -> Option<System> {
        let system = nif.get_as::<nif::ParticleSystem>(Some(at))?;
        let mut anchors = vec![*frames.0.get(&at)?];
        let mut anchor = |r: nif::Ref| -> Option<usize> {
            let a = *frames.0.get(&r?)?;
            anchors.push(a);
            Some(anchors.len() - 1)
        };
        let chain: Vec<&nif::Controller> = controllers(nif, system.shape.av.controller).collect();
        let timed = |kind: ControllerKind, name: &str| {
            chain
                .iter()
                .find(|c| c.kind == kind && c.modifier == name)
                .map(|c| (*c).clone())
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
                                x_axis: world_vec(*x_axis),
                                y_axis: world_vec(*y_axis),
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
                    axis: world_vec(g.axis).normalize_or_zero(),
                    decay: g.decay,
                    strength: g.strength,
                    spherical: g.force == 1,
                    turbulence: g.turbulence * g.turbulence_scale,
                },
                // Drag needs an object to measure its range from.
                ModifierKind::Drag(d) => match anchor(d.object) {
                    Some(a) => StepKind::Drag {
                        anchor: a,
                        axis: world_vec(d.axis).normalize_or_zero(),
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
                                            .map(|&p| world_vec(p))
                                            .collect(),
                                        normals: g.normals.iter().map(|&n| world_vec(n)).collect(),
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
                                axis: world_vec(*axis),
                            }
                        }
                    };
                    let keyed = |kind| timed(kind, &m.name);
                    StepKind::Emit(Box::new(Emitter {
                        settings: e.clone(),
                        shape,
                        keys: keyed(ControllerKind::Emitter).and_then(|c| {
                            let keys = nif.get_as::<nif::EmitterKeys>(c.data)?.clone();
                            Some(Timed {
                                controller: c,
                                keys,
                            })
                        }),
                        radius: keyed(ControllerKind::EmitterInitialRadius).and_then(|c| {
                            let keys = nif.get_as::<Keys<f32>>(c.data)?.clone();
                            Some(Timed {
                                controller: c,
                                keys,
                            })
                        }),
                        speed: keyed(ControllerKind::EmitterSpeed).and_then(|c| {
                            let keys = nif.get_as::<Keys<f32>>(c.data)?.clone();
                            Some(Timed {
                                controller: c,
                                keys,
                            })
                        }),
                        cycles: false,
                        reset: false,
                    }))
                }
                _ => continue,
            };
            let active = timed(ControllerKind::ModifierActive, &m.name).and_then(|c| {
                let keys = nif.get_as::<BoolKeys>(c.data)?.clone();
                Some(Timed {
                    controller: c,
                    keys,
                })
            });
            steps.push(Step {
                kind,
                active,
                on: m.active,
            });
        }
        let capacity = nif
            .get_as::<nif::ParticleData>(system.shape.data)
            .map_or(0, |d| d.capacity);
        let has_colours = steps.iter().any(|s| matches!(s.kind, StepKind::Colour(_)));
        steps
            .iter()
            .any(|s| matches!(s.kind, StepKind::Emit(_)))
            .then_some(System {
                anchors,
                world_space: system.world_space,
                capacity,
                steps,
                look: Look {
                    // Without colour keys the squares are white.
                    tinted: look.tinted && has_colours,
                    ..look
                },
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
        let mut anchor = |r: nif::Ref| -> Option<usize> {
            let a = *frames.0.get(&r?)?;
            anchors.push(a);
            Some(anchors.len() - 1)
        };
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
                        axis: world_vec(*axis).normalize_or_zero(),
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
                        axis: world_vec(*direction).normalize_or_zero(),
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
        let has_colours = kinds.iter().any(|k| matches!(k, StepKind::Colour(_)));
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
            look: Look {
                tinted: look.tinted && has_colours,
                ..look
            },
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

/// A vector in the model's z-up axes, in Bevy's.
fn world_vec([x, y, z]: [f32; 3]) -> Vec3 {
    world(x, y, z)
}

/// Spawns a model's particle systems `offset` seconds into its animation:
/// their anchors under `root`, or under the entity `node` gives for a nif
/// node where a rig has one.
pub(crate) fn spawn(
    commands: &mut Commands,
    systems: &[Arc<System>],
    root: Entity,
    node: impl Fn(usize) -> Option<Entity>,
    offset: f32,
) {
    // By the object too, so effects that start together don't play alike.
    let object = root.to_bits();
    let seed = offset.to_bits() ^ object as u32 ^ (object >> 32) as u32;
    for (i, system) in systems.iter().enumerate() {
        let anchors: Vec<Entity> = system
            .anchors
            .iter()
            .map(|a| {
                let (parent, transform) = match node(a.node) {
                    Some(parent) => (parent, bevy_transform(&a.below)),
                    None => (root, bevy_transform(&a.model)),
                };
                commands
                    .spawn((transform, Visibility::default(), ChildOf(parent)))
                    .id()
            })
            .collect();
        commands.entity(anchors[0]).insert(Particles {
            system: system.clone(),
            anchors,
            state: State::new(seed.wrapping_mul(31).wrapping_add(i as u32), offset),
            copies: Vec::new(),
            rate: 1.0,
        });
    }
}

/// A particle system playing. Its squares are drawn by a [`Batch`].
#[derive(Component)]
pub struct Particles {
    system: Arc<System>,
    anchors: Vec<Entity>,
    state: State,
    /// The entity showing each mesh particle's copy, by particle.
    copies: Vec<(u32, Entity)>,
    /// Seconds of its animation per second played.
    pub(crate) rate: f32,
}

/// Marks the entity a mesh particle's copy hangs from.
#[derive(Component)]
pub struct MeshCopy;

/// Gives each mesh particle its copy of its meshes as it's born, takes it
/// away when it dies, and puts it where the particle is, turned and sized
/// as it is. Runs after [`play`] and before anything is drawn, so it places
/// the copies itself rather than waiting for transforms to be propagated.
pub fn place_copies(
    mut commands: Commands,
    mut systems: Query<(&mut Particles, &GlobalTransform)>,
    mut copies: Query<(&mut Transform, &mut GlobalTransform, &Children), CopyParts>,
    mut parts: Query<(&Transform, &mut GlobalTransform), CopyMeshes>,
    mut stores: MaterialStores,
) {
    let mut materials = stores.parts();
    for (mut particles, home) in &mut systems {
        let particles = &mut *particles;
        if particles.system.copies.is_empty() {
            continue;
        }
        let state = &particles.state;
        // Both are in order of birth, so they're matched up in one pass.
        let mut shown = std::mem::take(&mut particles.copies).into_iter().peekable();
        for p in &state.particles {
            while let Some((_, e)) = shown.next_if(|&(id, _)| id < p.id) {
                commands.entity(e).despawn();
            }
            let transform = Transform {
                translation: p.position,
                rotation: p
                    .axis
                    .filter(|a| *a != Vec3::ZERO)
                    .map_or(Quat::IDENTITY, |a| {
                        Quat::from_axis_angle(a.normalize(), p.angle)
                    }),
                scale: Vec3::splat(p.radius * p.size),
            };
            let global = *home * transform;
            if let Some((id, e)) = shown.next_if(|&(id, _)| id == p.id) {
                if let Ok((mut t, mut g, children)) = copies.get_mut(e) {
                    *t = transform;
                    *g = global;
                    for &child in children {
                        if let Ok((t, mut g)) = parts.get_mut(child) {
                            *g = global * *t;
                        }
                    }
                }
                particles.copies.push((id, e));
                continue;
            }
            let meshes = &particles.system.copies;
            let meshes = &meshes[(p.generation as usize).min(meshes.len() - 1)];
            // Their own controllers play on the system's clock.
            let rate = particles.rate * particles.system.clock;
            let copy = commands
                .spawn((
                    transform,
                    global,
                    Visibility::default(),
                    MeshCopy,
                    ChildOf(particles.anchors[0]),
                ))
                .id();
            for part in meshes.iter() {
                let painted = part.animation.as_ref().is_some_and(|a| a.paints());
                let material = match painted {
                    true => materials.copy(&part.material),
                    false => part.material.clone(),
                };
                let animated = part.animation.as_ref().map(|animation| {
                    Animated::at_rate(animation.clone(), state.time, rate, material.clone())
                });
                let posed = animated.as_ref().and_then(|a| a.pose(&mut materials));
                let t = posed.unwrap_or_default();
                let mut child =
                    commands.spawn((Mesh3d(part.mesh.clone()), t, global * t, ChildOf(copy)));
                if part.blended {
                    child.insert(NotShadowCaster);
                }
                material.insert(&mut child);
                if let Some(animated) = animated {
                    child.insert(animated);
                }
            }
            particles.copies.push((p.id, copy));
        }
        for (_, e) in shown {
            commands.entity(e).despawn();
        }
    }
}

/// The entities showing mesh particles' copies.
type CopyParts = (With<MeshCopy>, Without<Particles>);

/// The meshes of those copies, among others.
type CopyMeshes = (Without<MeshCopy>, Without<Particles>);

/// Marks the entities particles' squares are drawn by.
#[derive(Component)]
pub struct ParticleMesh;

/// What changes as a system plays.
struct State {
    /// Seconds into its model's animation, which its keys are read at.
    time: f32,
    /// Particles owed to the birth rate since the emitter came on.
    owed: f32,
    played: bool,
    particles: Vec<Particle>,
    born: u32,
    rng: Rng,
}

impl State {
    /// A system `start` seconds into its model's animation.
    fn new(seed: u32, start: f32) -> State {
        State {
            time: start,
            owed: 0.0,
            played: false,
            particles: Vec::new(),
            born: 0,
            rng: Rng(u64::from(seed) ^ 0x9e37_79b9_7f4a_7c15),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Particle {
    /// In world space for a world-space system, else the system's own.
    position: Vec3,
    velocity: Vec3,
    age: f32,
    life: f32,
    radius: f32,
    size: f32,
    colour: [f32; 4],
    generation: u16,
    /// Which particle of its system it is.
    id: u32,
    /// What it turns about, once something turns it, and how far.
    axis: Option<Vec3>,
    angle: f32,
    /// Born this frame, already moved on by its age.
    new: bool,
    /// Seconds of the frame left after it hit something this frame.
    left: Option<f32>,
}

/// Where the anchors are this frame, in the space particles are kept in.
struct Space {
    anchors: Vec<Affine3A>,
}

impl System {
    /// Runs its modifiers over `dt` seconds.
    fn step(&self, state: &mut State, space: &Space, dt: f32) {
        let before = state.time;
        state.time += dt;
        let t = state.time;
        for p in &mut state.particles {
            p.new = false;
            p.left = None;
        }
        for step in &self.steps {
            // An emitter switched off still emits: its own keys decide.
            let on = match (&step.active, &step.kind) {
                (_, StepKind::Emit(_)) => true,
                (Some(a), _) => {
                    on_since(&a.keys, a.time(before), a.time(t), a.loops()).unwrap_or(step.on)
                }
                (None, _) => step.on,
            };
            if !on {
                continue;
            }
            match &step.kind {
                StepKind::Age => {
                    for p in state.particles.iter_mut().filter(|p| !p.new) {
                        p.age += dt;
                    }
                    state.particles.retain(|p| p.age <= p.life);
                }
                StepKind::Emit(e) => self.emit(e, state, space, dt),
                StepKind::Gravity {
                    anchor,
                    axis,
                    decay,
                    strength,
                    spherical,
                    turbulence,
                } => {
                    let frame = space.anchors[*anchor];
                    let origin = Vec3::from(frame.translation);
                    let axis = frame.transform_vector3(*axis).normalize_or_zero();
                    for p in &mut state.particles {
                        let (direction, distance) = match spherical {
                            true => {
                                let to = origin - p.position;
                                (to.normalize_or_zero(), to.length())
                            }
                            false => (axis, (p.position - origin).dot(axis).abs()),
                        };
                        p.velocity += direction * strength * 1.6 * (-decay * distance).exp() * dt;
                        if *turbulence != 0.0 {
                            let jolt = Vec3::new(
                                state.rng.signed(),
                                state.rng.signed(),
                                state.rng.signed(),
                            );
                            p.velocity += jolt * turbulence * 500.0 * dt;
                        }
                    }
                }
                StepKind::Drag {
                    anchor,
                    axis,
                    percentage,
                    range,
                    falloff,
                } => {
                    let frame = space.anchors[*anchor];
                    let origin = Vec3::from(frame.translation);
                    let axis = frame.transform_vector3(*axis).normalize_or_zero();
                    for p in &mut state.particles {
                        let distance = p.position.distance(origin);
                        let reach = if distance <= *range {
                            1.0
                        } else if distance >= *falloff {
                            0.0
                        } else {
                            (falloff - distance) / (falloff - range)
                        };
                        let along = p.velocity.dot(axis);
                        let shrink = (30.0 * percentage * dt * reach).clamp(0.0, 1.0);
                        p.velocity -= axis * along * shrink;
                    }
                }
                StepKind::Move => {
                    for p in state.particles.iter_mut().filter(|p| !p.new) {
                        p.position += p.velocity * p.left.unwrap_or(dt);
                    }
                }
                StepKind::Collide(colliders) => {
                    let frames: Vec<Affine3A> =
                        colliders.iter().map(|c| space.anchors[c.anchor]).collect();
                    state.particles.retain_mut(|p| {
                        if p.new {
                            return true;
                        }
                        let hit = colliders
                            .iter()
                            .zip(&frames)
                            .filter_map(|(c, f)| Some((c, c.hit(f, p, dt)?)))
                            .min_by(|a, b| a.1.at.total_cmp(&b.1.at));
                        let Some((c, hit)) = hit else { return true };
                        if c.die {
                            return false;
                        }
                        p.position = hit.point;
                        let across = p.velocity.dot(hit.normal);
                        p.velocity = match hit.settle {
                            true => p.velocity - hit.normal * across,
                            false => (p.velocity - hit.normal * 2.0 * across) * c.bounce,
                        };
                        p.left = Some(dt * (1.0 - hit.at));
                        true
                    });
                }
                StepKind::GrowFade(g) => {
                    for p in &mut state.particles {
                        p.size = grow_fade(g, p);
                    }
                }
                StepKind::Colour(keys) => {
                    for p in &mut state.particles {
                        let u = if p.life > 0.0 { p.age / p.life } else { 1.0 };
                        if let Some(c) = keys.sample(u) {
                            p.colour = c.map(|v| v.clamp(0.0, 1.0));
                        }
                    }
                }
                StepKind::Spin {
                    random,
                    axis,
                    speed,
                } => {
                    for p in &mut state.particles {
                        if p.axis.is_none() {
                            p.axis = Some(match random {
                                true => random_axis(&mut state.rng),
                                false => *axis,
                            });
                        }
                        let turned = if p.new { p.age } else { dt };
                        p.angle = (p.angle + speed * turned).rem_euclid(TAU);
                    }
                }
            }
        }
        state.played = true;
    }

    /// Bears the particles owed over the last `dt` seconds.
    fn emit(&self, e: &Emitter, state: &mut State, space: &Space, dt: f32) {
        let t = state.time;
        let (on, rate) = match &e.keys {
            Some(k) => {
                let kt = k.time(t);
                (
                    bool_at(&k.keys.active, kt).unwrap_or(true),
                    k.keys.birth_rate.sample(kt).unwrap_or(0.0),
                )
            }
            None => (false, 0.0),
        };
        if let Some(k) = e.keys.as_ref().filter(|_| e.cycles)
            && k.wraps(t - dt, t)
        {
            if e.reset {
                state.particles.clear();
            }
            // What came due since the cycle started is never born.
            state.owed = ((k.time(t) - k.controller.start) * rate).fract();
            return;
        }
        if !on || rate <= 0.0 {
            state.owed = 0.0;
            return;
        }
        // Nothing is born on a system's first frame.
        if !state.played {
            return;
        }
        state.owed += rate * dt;
        let due = state.owed.floor();
        state.owed -= due;
        let mut settings = e.settings.clone();
        if let Some(r) = &e.radius {
            settings.radius = r.keys.sample(r.time(t)).unwrap_or(settings.radius);
        }
        if let Some(s) = &e.speed {
            settings.speed = s.keys.sample(s.time(t)).unwrap_or(settings.speed);
        }
        for k in 0..due as usize {
            if state.particles.len() >= self.capacity {
                break;
            }
            // The k-th of those owed, last born youngest.
            let age = (state.owed + (due - 1.0 - k as f32)) / rate;
            let mut p = birth(&settings, &e.shape, space, &mut state.rng);
            p.age = age;
            p.id = state.born;
            state.born = state.born.wrapping_add(1);
            p.position += p.velocity * age;
            state.particles.push(p);
        }
    }
}

/// Whether on/off keys are on at key time `t`, or turned on at any point
/// since `before`: wrapping round from the end to the start if they
/// `loop`, else playing backwards.
fn on_since(keys: &BoolKeys, before: f32, t: f32, looping: bool) -> Option<bool> {
    let passed = |at: f32| match (before <= t, looping) {
        (true, _) => at > before && at <= t,
        (false, true) => at > before || at <= t,
        (false, false) => at >= t && at < before,
    };
    let switched_on = keys.iter().any(|&(at, on)| on && passed(at));
    Some(switched_on || bool_at(keys, t)?)
}

/// The value of on/off keys at `t`: the last key at or before it.
fn bool_at(keys: &BoolKeys, t: f32) -> Option<bool> {
    keys.iter()
        .take_while(|(at, _)| *at <= t)
        .last()
        .or(keys.first())
        .map(|(_, on)| *on)
}

/// A particle's size from its age, for one of the generation it acts on.
fn grow_fade(g: &nif::GrowFade, p: &Particle) -> f32 {
    let grow = (g.grow > 0.0 && p.age < g.grow && p.generation == g.grow_generation)
        .then(|| p.age / g.grow);
    let left = p.life - p.age;
    let fade =
        (g.fade > 0.0 && left < g.fade && p.generation == g.fade_generation).then(|| left / g.fade);
    let size = match (grow, fade) {
        (Some(a), Some(b)) => a.min(b),
        (Some(s), None) | (None, Some(s)) => s,
        (None, None) => 1.0,
    };
    size.max(0.0001)
}

/// A new particle as `e` bears it.
fn birth(e: &nif::Emitter, shape: &Shape, space: &Space, rng: &mut Rng) -> Particle {
    let speed = e.speed + rng.signed() * e.speed_variation / 2.0;
    let declination = e.declination + rng.signed() * e.declination_variation;
    let planar = e.planar_angle + rng.signed() * e.planar_angle_variation;
    // From the emitter's +z, turned about it from its +x.
    let direction = world_vec([
        declination.sin() * planar.cos(),
        declination.sin() * planar.sin(),
        declination.cos(),
    ]);
    let life = e.life_span + rng.signed() * e.life_span_variation / 2.0;
    let (position, velocity) = match shape {
        Shape::Box { anchor, size } => {
            let at = Vec3::new(rng.signed(), rng.signed(), rng.signed())
                * world_vec(size.to_array()).abs()
                / 2.0;
            placed(space.anchors[*anchor], at, direction * speed)
        }
        Shape::Cylinder {
            anchor,
            radius,
            height,
        } => {
            let angle = rng.unit() * TAU;
            let r = rng.unit() * radius;
            let h = rng.signed() * height / 2.0;
            let at = world_vec([r * angle.cos(), r * angle.sin(), h]);
            placed(space.anchors[*anchor], at, direction * speed)
        }
        Shape::Sphere { anchor, radius } => {
            let (r, a, b) = (rng.unit() * radius, rng.unit() * TAU, rng.unit() * TAU);
            let at = world_vec([r * a.cos() * b.sin(), r * a.sin() * b.sin(), r * b.cos()]);
            placed(space.anchors[*anchor], at, direction * speed)
        }
        Shape::Mesh {
            meshes,
            velocity,
            emission,
            axis,
        } => {
            let mesh = &meshes[rng.below(meshes.len())];
            let (at, normal) = mesh.spot(*emission, rng);
            let frame = space.anchors[mesh.anchor];
            let flight = match (velocity, normal) {
                (0, Some(n)) => n * speed,
                (2, _) => {
                    let fresh = e.speed + rng.signed() * e.speed_variation / 2.0;
                    *axis * fresh
                }
                _ => direction * speed,
            };
            placed(frame, at, flight)
        }
    };
    Particle {
        position,
        velocity,
        age: 0.0,
        life,
        radius: e.radius,
        size: 1.0,
        colour: e.color,
        generation: 0,
        id: 0,
        axis: None,
        angle: 0.0,
        new: true,
        left: None,
    }
}

/// An axis with its z a random cos(πu) and its direction about z a random
/// 2πv, in Bevy's axes.
fn random_axis(rng: &mut Rng) -> Vec3 {
    let z = (std::f32::consts::PI * rng.unit()).cos();
    let about = TAU * rng.unit();
    let r = (1.0 - z * z).max(0.0).sqrt();
    world_vec([r * about.cos(), r * about.sin(), z])
}

/// Where a particle's path over a frame meets a collider.
struct Hit {
    /// The fraction of the frame it took.
    at: f32,
    point: Vec3,
    /// The surface's, to mirror the velocity about.
    normal: Vec3,
    /// It stops crossing the surface rather than bouncing.
    settle: bool,
}

impl Collider {
    fn hit(&self, frame: &Affine3A, p: &Particle, dt: f32) -> Option<Hit> {
        let centre = Vec3::from(frame.translation);
        let scale = frame.matrix3.x_axis.length();
        let (from, path) = (p.position, p.velocity * dt);
        match self.shape {
            ColliderShape::Plane {
                x_axis,
                y_axis,
                width,
                height,
            } => {
                let x = frame.transform_vector3(x_axis).normalize_or_zero();
                let y = frame.transform_vector3(y_axis).normalize_or_zero();
                let normal = x.cross(y).normalize_or_zero();
                let start = (from - centre).dot(normal);
                let end = (from + path - centre).dot(normal);
                let at = if start.abs() <= 0.001 {
                    0.0
                } else if (start > 0.0) != (end > 0.0) || end == 0.0 {
                    start / (start - end)
                } else {
                    return None;
                };
                let point = from + path * at;
                let off = point - centre;
                let reach = std::f32::consts::FRAC_1_SQRT_2 * scale;
                if off.dot(x).abs() > width * reach || off.dot(y).abs() > height * reach {
                    return None;
                }
                let away = p.velocity.dot(normal) * start.signum() > 0.001;
                Some(Hit {
                    at,
                    point,
                    normal,
                    settle: at >= 1.0 && !away,
                })
            }
            ColliderShape::Sphere { radius } => {
                let r2 = (radius * scale).powi(2);
                let rel = from - centre;
                let gap = rel.length_squared() - r2;
                if gap.abs() <= 0.001 {
                    return None;
                }
                // |rel + path s|² = r², for s from 0 to 1.
                let a = path.length_squared();
                if a == 0.0 {
                    return None;
                }
                let b = 2.0 * rel.dot(path);
                let root = (b * b - 4.0 * a * gap).sqrt();
                if root.is_nan() {
                    return None;
                }
                // Outside it enters, inside it leaves.
                let at = match gap > 0.0 {
                    true => (-b - root) / (2.0 * a),
                    false => (-b + root) / (2.0 * a),
                };
                if !(0.0..=1.0).contains(&at) {
                    return None;
                }
                let point = from + path * at;
                Some(Hit {
                    at,
                    point,
                    normal: (point - centre).normalize_or_zero(),
                    settle: false,
                })
            }
        }
    }
}

/// A spot and a flight in `frame`'s space, in the particles' space.
fn placed(frame: Affine3A, at: Vec3, flight: Vec3) -> (Vec3, Vec3) {
    let speed = flight.length();
    let direction = frame.transform_vector3(flight).normalize_or_zero();
    (frame.transform_point3(at), direction * speed)
}

impl EmitterMesh {
    /// Where on the mesh a particle starts by the emission type, and the
    /// normal there if the mesh has normals.
    fn spot(&self, emission: u32, rng: &mut Rng) -> (Vec3, Option<Vec3>) {
        let normal = |i: u16| self.normals.get(i as usize).copied();
        let corner = |i: u16| self.positions.get(i as usize).copied().unwrap_or_default();
        match emission {
            1 | 3 if !self.triangles.is_empty() => {
                let [a, b, c] = self.triangles[rng.below(self.triangles.len())];
                let n = match (normal(a), normal(b), normal(c)) {
                    (Some(x), Some(y), Some(z)) => Some((x + y + z).normalize_or_zero()),
                    _ => None,
                };
                let at = match emission {
                    1 => (corner(a) + corner(b) + corner(c)) / 3.0,
                    _ => {
                        let u = rng.unit();
                        let v = rng.unit() * (1.0 - u);
                        corner(a) + (corner(b) - corner(a)) * u + (corner(c) - corner(a)) * v
                    }
                };
                (at, n)
            }
            _ => {
                let i = rng.below(self.positions.len()) as u16;
                (corner(i), normal(i))
            }
        }
    }
}

/// A small random number generator, so each system plays its own way.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in [-1, 1).
    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// Uniform below `n`, which isn't 0.
    fn below(&mut self, n: usize) -> usize {
        (self.unit() * n as f32) as usize % n.max(1)
    }
}

/// The 3D cameras, which aren't particle meshes.
type Cameras = (With<Camera3d>, Without<ParticleMesh>);

/// Plays every particle system. Runs once transforms are propagated, so it
/// sees where the anchors are this frame.
pub fn play(
    time: Res<Time>,
    mut systems: Query<(&mut Particles, &GlobalTransform), Without<ParticleMesh>>,
    anchors: Query<&GlobalTransform, Without<ParticleMesh>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (mut particles, own) in &mut systems {
        let particles = &mut *particles;
        let own = own.affine();
        let to_space = match particles.system.world_space {
            true => Affine3A::IDENTITY,
            false => own.inverse(),
        };
        let space = Space {
            anchors: particles
                .anchors
                .iter()
                .map(|&a| to_space * anchors.get(a).map_or(own, |g| g.affine()))
                .collect(),
        };
        let dt = dt * particles.rate * particles.system.clock;
        particles.system.step(&mut particles.state, &space, dt);
    }
}

/// Every particle texture, as a layer of one array, so one material draws
/// every system and all their squares go in one mesh, in the order they're
/// drawn in.
pub(crate) const TEXTURES: Handle<Image> = uuid_handle!("6b0c3f0e-8d0a-4e57-9a8f-2f3c1d6e9b41");

/// The side of each layer of [`TEXTURES`], which every particle texture is
/// resampled to: the largest they come in.
const SIDE: usize = 128;

/// The particle textures loaded so far, as layers of [`TEXTURES`]: white
/// first, for systems with none.
pub(crate) struct Layers {
    /// By where each was loaded from: none if it couldn't be.
    by_source: HashMap<TextureSource, Option<u32>>,
    /// Every layer's pixels and mipmaps, one after another, as the array
    /// takes them.
    data: Vec<u8>,
    count: u32,
    mips: u32,
}

impl Default for Layers {
    fn default() -> Self {
        let white = white();
        Layers {
            by_source: HashMap::new(),
            mips: white.texture_descriptor.mip_level_count,
            data: white.data.unwrap_or_default(),
            count: 1,
        }
    }
}

impl Layers {
    /// The layer of the texture from `source`, loaded by `load` the first
    /// time: none if it can't be.
    pub fn layer(
        &mut self,
        source: &TextureSource,
        images: &mut Assets<Image>,
        load: impl FnOnce() -> Option<Image>,
    ) -> Option<u32> {
        if let Some(&layer) = self.by_source.get(source) {
            return layer;
        }
        let layer = load().and_then(resampled).and_then(|i| i.data).map(|data| {
            self.data.extend(data);
            self.count += 1;
            // Bevy can't add a layer in place, so the whole array goes up
            // again, but only as models load.
            _ = images.insert(&TEXTURES, self.array());
            self.count - 1
        });
        self.by_source.insert(source.clone(), layer);
        layer
    }

    /// Every layer so far, as an array texture.
    fn array(&self) -> Image {
        let side = SIDE as u32;
        let format = TextureFormat::Rgba8UnormSrgb;
        blend::array_image(
            self.data.clone(),
            (side, side),
            self.mips,
            format,
            self.count,
        )
    }
}

/// A layer of plain white.
fn white() -> Image {
    terrain::layer_image(vec![255; SIDE * SIDE * 4], SIDE)
}

/// `image` as a layer of [`TEXTURES`]: bilinearly resampled, as it would be
/// sampled at its own size, and mipmapped.
fn resampled(image: Image) -> Option<Image> {
    let image = textures::decompress(image)?.convert(TextureFormat::Rgba8UnormSrgb)?;
    let (w, h) = (image.width() as usize, image.height() as usize);
    let src = image.data?;
    let texel = |x: isize, y: isize, c: usize| {
        let (x, y) = (
            x.rem_euclid(w as isize) as usize,
            y.rem_euclid(h as isize) as usize,
        );
        src[(y * w + x) * 4 + c] as f32
    };
    let mut data = Vec::with_capacity(SIDE * SIDE * 4);
    for i in 0..SIDE * SIDE {
        // Where this texel's centre falls among the source's, which repeat.
        let u = ((i % SIDE) as f32 + 0.5) * w as f32 / SIDE as f32 - 0.5;
        let v = ((i / SIDE) as f32 + 0.5) * h as f32 / SIDE as f32 - 0.5;
        let (x, y) = (u.floor() as isize, v.floor() as isize);
        let (fx, fy) = (u - u.floor(), v - v.floor());
        for c in 0..4 {
            let top = texel(x, y, c) * (1.0 - fx) + texel(x + 1, y, c) * fx;
            let bottom = texel(x, y + 1, c) * (1.0 - fx) + texel(x + 1, y + 1, c) * fx;
            data.push((top * (1.0 - fy) + bottom * fy).round() as u8);
        }
    }
    Some(terrain::layer_image(data, SIDE))
}

/// Draws every system's squares, whatever its texture and blending, with
/// [`TEXTURES`].
pub type ParticleMaterial = ExtendedMaterial<StandardMaterial, ParticleTextures>;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct ParticleTextures {
    #[texture(100, dimension = "2d_array")]
    #[sampler(101)]
    textures: Handle<Image>,
}

impl MaterialExtension for ParticleTextures {
    fn vertex_shader() -> ShaderRef {
        "embedded://tgrender/particles.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://tgrender/particles.wesl".into()
    }
}

/// The meshes every system's squares are drawn in: one for each layer the
/// cameras draw. A mesh for each system, refilled every frame, cost more to
/// upload and draw than everything else the systems do.
#[derive(Resource, Default)]
pub struct Draws {
    material: Option<Handle<ParticleMaterial>>,
    /// How many layers [`TEXTURES`] had when the material last bound it.
    layers: u32,
    draws: Vec<Draw>,
    /// Kept between frames for their room.
    squares: Vec<Square>,
    systems: Vec<Drawn>,
}

/// The mesh drawing the systems in one layer, as its camera sees them.
struct Draw {
    layers: Option<RenderLayers>,
    entity: Entity,
    mesh: Handle<Mesh>,
    /// Spawned this frame, so its entity isn't there to place yet.
    new: bool,
    /// Its mesh holds squares, so it has to be emptied once it has none.
    filled: bool,
    view: Option<Eye>,
    /// Its systems' squares this frame, in the order they're drawn.
    runs: Vec<Range<usize>>,
}

/// Where a camera is and which way it looks.
#[derive(Clone, Copy)]
struct Eye {
    at: Vec3,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
}

/// A system's squares this frame.
struct Drawn {
    draw: usize,
    /// How far in front of the camera the middle of its squares is.
    depth: f32,
    squares: Range<usize>,
}

/// A particle's square, in the world.
#[derive(Clone, Copy)]
struct Square {
    at: Vec3,
    half: f32,
    colour: [f32; 4],
    /// Its system's look, as the shader takes it: its texture layer and how
    /// it blends and is lit, and its emissive colour and alpha cutoff.
    look: [f32; 2],
    extra: [f32; 4],
}

/// What [`draw`] needs of a system.
type SystemLook = (
    &'static Particles,
    &'static GlobalTransform,
    &'static InheritedVisibility,
    Option<&'static RenderLayers>,
);

/// The assets [`draw`] makes and changes.
#[derive(SystemParam)]
pub struct DrawStores<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    images: ResMut<'w, Assets<Image>>,
    materials: ResMut<'w, Assets<ParticleMaterial>>,
}

/// What [`draw`] needs of a camera.
type CameraLook = (
    &'static Camera,
    &'static GlobalTransform,
    Option<&'static RenderLayers>,
);

/// Redraws every system's squares to face the camera that sees them. Runs
/// once visibility is propagated, so hidden systems are left out, and
/// before what each camera sees is worked out, so a new mesh is drawn at
/// once.
pub fn draw(
    mut commands: Commands,
    mut draws: ResMut<Draws>,
    systems: Query<SystemLook, Without<ParticleMesh>>,
    cameras: Query<CameraLook, Cameras>,
    mut placed: Query<(&mut Transform, &mut GlobalTransform), With<ParticleMesh>>,
    mut stores: DrawStores,
) {
    let DrawStores {
        meshes,
        images,
        materials,
    } = &mut stores;
    let textures = match images.get(&TEXTURES) {
        Some(image) => image.texture_descriptor.size.depth_or_array_layers,
        None => {
            _ = images.insert(&TEXTURES, Layers::default().array());
            1
        }
    };
    let Draws {
        material,
        layers,
        draws,
        squares,
        systems: drawn,
    } = &mut *draws;
    let material = material.get_or_insert_with(|| {
        materials.add(ParticleMaterial {
            base: StandardMaterial {
                alpha_mode: AlphaMode::Premultiplied,
                double_sided: true,
                cull_mode: None,
                // Without highlights, as the game draws them.
                perceptual_roughness: 1.0,
                reflectance: 0.0,
                ..default()
            },
            extension: ParticleTextures { textures: TEXTURES },
        })
    });
    // A larger array is a new texture, which the material binds only once
    // it's changed itself.
    if std::mem::replace(layers, textures) != textures {
        materials.get_mut(&*material).map(AssetMut::into_inner);
    }
    let eye = |layers: Option<&RenderLayers>| {
        let layers = layers.unwrap_or(&RenderLayers::layer(0)).clone();
        let (_, camera, _) = cameras.iter().find(|(c, _, seen)| {
            c.is_active && seen.unwrap_or(&RenderLayers::layer(0)).intersects(&layers)
        })?;
        Some(Eye {
            at: camera.translation(),
            forward: camera.forward().as_vec3(),
            right: camera.right().as_vec3(),
            up: camera.up().as_vec3(),
        })
    };
    for draw in draws.iter_mut() {
        draw.view = eye(draw.layers.as_ref());
    }
    // The mesh drawing `layers`, and the camera that sees it.
    let mut draw_for = |layers: Option<&RenderLayers>| {
        let i = match draws.iter().position(|d| d.layers.as_ref() == layers) {
            Some(i) => i,
            None => {
                let mut draw = spawn_draw(&mut commands, meshes, material, layers);
                draw.view = eye(layers);
                draws.push(draw);
                draws.len() - 1
            }
        };
        (i, draws[i].view)
    };
    // Made at once, so its pipeline is ready for the first square.
    draw_for(None);
    drawn.clear();
    squares.clear();
    for (particles, own, shown, layers) in &systems {
        let (system, state) = (&particles.system, &particles.state);
        // Mesh particles draw as copies of their meshes instead.
        if !system.copies.is_empty() || !shown.get() || state.particles.is_empty() {
            continue;
        }
        let (draw, Some(view)) = draw_for(layers) else {
            continue;
        };
        let own = own.affine();
        let to_world = match system.world_space {
            true => Affine3A::IDENTITY,
            false => own,
        };
        let scale = own.matrix3.x_axis.length();
        let look = &system.look;
        let (mode, cutoff) = match look.blend {
            Blend::Over => (0, 0.0),
            Blend::Add => (1, 0.0),
            Blend::Opaque { cutoff } => (2, cutoff),
        };
        let flags = mode + if look.lit { 4 } else { 0 };
        let [er, eg, eb] = look.emissive;
        let start = squares.len();
        squares.extend(state.particles.iter().map(|p| Square {
            at: to_world.transform_point3(p.position),
            half: p.radius * p.size * scale,
            colour: match look.tinted {
                // The model's colours are sRGB, as everywhere else.
                true => {
                    let [r, g, b, a] = p.colour;
                    let c = Color::srgba(r, g, b, a).to_linear().to_f32_array();
                    std::array::from_fn(|i| c[i] * look.colour[i])
                }
                false => look.colour,
            },
            look: [look.layer as f32, flags as f32],
            extra: [er, eg, eb, cutoff],
        }));
        drawn.push(Drawn {
            draw,
            depth: depth(&squares[start..], &view),
            squares: start..squares.len(),
        });
    }
    far_to_near(drawn);
    for system in drawn.iter() {
        draws[system.draw].runs.push(system.squares.clone());
    }
    draws.retain_mut(|draw| {
        let new = std::mem::take(&mut draw.new);
        let runs = std::mem::take(&mut draw.runs);
        let Some(view) = draw.view.filter(|_| !runs.is_empty() || draw.filled) else {
            return true;
        };
        // At the camera, so it's sorted after every other blended model.
        let at = Transform::from_translation(view.at);
        match placed.get_mut(draw.entity) {
            // Propagation has run, so its place this frame is set here.
            Ok((mut t, mut g)) => (*t, *g) = (at, at.into()),
            Err(_) if new => {
                commands
                    .entity(draw.entity)
                    .insert((at, GlobalTransform::from(at)));
            }
            // Something despawned it, so another is made when needed.
            Err(_) => return false,
        }
        let mut ordered = runs.iter().flat_map(|r| &squares[r.clone()]);
        if let Some(mut mesh) = meshes.get_mut(&draw.mesh) {
            fill(&mut mesh, &mut ordered, &view);
        }
        draw.filled = !runs.is_empty();
        // Kept for its room.
        draw.runs = runs;
        draw.runs.clear();
        true
    });
}

/// How far in front of `view` the middle of the box around `squares` is,
/// which systems are drawn far to near by.
fn depth(squares: &[Square], view: &Eye) -> f32 {
    let (low, high) = squares.iter().fold((Vec3::MAX, Vec3::MIN), |(l, h), s| {
        (l.min(s.at), h.max(s.at))
    });
    ((low + high) / 2.0 - view.at).dot(view.forward)
}

/// Puts systems in the order they're drawn: far to near, as blended squares
/// cover what's drawn before them.
fn far_to_near(systems: &mut [Drawn]) {
    systems.sort_by(|a, b| b.depth.total_cmp(&a.depth));
}

/// Spawns the entity drawing the squares in `layers`.
fn spawn_draw(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<ParticleMaterial>,
    layers: Option<&RenderLayers>,
) -> Draw {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, Vec::<[f32; 2]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, Vec::<[f32; 4]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new())
    .with_inserted_indices(Indices::U32(Vec::new()));
    let nowhere = Eye {
        at: Vec3::ZERO,
        forward: Vec3::NEG_Z,
        right: Vec3::X,
        up: Vec3::Y,
    };
    fill(&mut mesh, &mut std::iter::empty(), &nowhere);
    let mesh = meshes.add(mesh);
    let mut entity = commands.spawn((
        Name::new("particles"),
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        // Visibility has been propagated this frame, and it's drawn from
        // its first.
        Visibility::Visible,
        InheritedVisibility::VISIBLE,
        ParticleMesh,
        // Its bounds change every frame.
        NoFrustumCulling,
        NotShadowCaster,
    ));
    if let Some(layers) = layers {
        entity.insert(layers.clone());
    }
    Draw {
        layers: layers.cloned(),
        entity: entity.id(),
        mesh,
        new: true,
        filled: false,
        view: None,
        runs: Vec::new(),
    }
}

/// A square's corners, across and up the screen, in drawing order.
const CORNERS: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

/// Refills a mesh in place: a square for each of `squares`, facing `view`
/// and placed from where it is. With no squares it holds one empty
/// triangle, as Bevy can't upload a mesh without vertices.
fn fill<'a>(mesh: &mut Mesh, squares: &mut (impl Iterator<Item = &'a Square> + Clone), view: &Eye) {
    use bevy::mesh::VertexAttributeValues as V;
    let n = squares.clone().count();
    // Each square's value at each of its corners, or a value for each
    // corner of the empty triangle, then for each of the two vertices that
    // centre the mesh's box.
    fn corners<'a, T: Copy>(
        v: &mut Vec<T>,
        squares: &(impl Iterator<Item = &'a Square> + Clone),
        f: impl Fn(&Square) -> [T; 4],
        none: T,
    ) {
        v.clear();
        v.extend(squares.clone().flat_map(&f));
        if v.is_empty() {
            v.extend([none; 3]);
        }
        v.extend([none; 2]);
    }
    let (right, up) = (view.right, view.up);
    if let Some(V::Float32x3(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
        corners(
            v,
            squares,
            |s| {
                let at = s.at - view.at;
                CORNERS.map(|(x, y)| (at + (right * x + up * y) * s.half).to_array())
            },
            [0.0; 3],
        );
        // Bevy sorts a blended mesh by the middle of its box, not where it's
        // placed, so two vertices no square uses put that middle at the
        // camera.
        let reach = v
            .iter()
            .fold(Vec3::ZERO, |r, &p| r.max(Vec3::from(p).abs()));
        let last = v.len() - 1;
        v[last - 1] = (-reach).to_array();
        v[last] = reach.to_array();
    }
    if let Some(V::Float32x3(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL) {
        corners(v, squares, |_| [(-view.forward).to_array(); 4], [0.0; 3]);
    }
    if let Some(V::Float32x2(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        corners(v, squares, |_| UVS, [0.0; 2]);
    }
    if let Some(V::Float32x2(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_1) {
        corners(v, squares, |s| [s.look; 4], [0.0; 2]);
    }
    if let Some(V::Float32x4(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_TANGENT) {
        corners(v, squares, |s| [s.extra; 4], [0.0; 4]);
    }
    if let Some(V::Float32x4(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        corners(v, squares, |s| [s.colour; 4], [0.0; 4]);
    }
    if let Some(Indices::U32(v)) = mesh.indices_mut() {
        v.clear();
        match n {
            0 => v.extend([0, 1, 2]),
            n => v.extend((0..n as u32).flat_map(|q| {
                let b = q * 4;
                [b, b + 1, b + 2, b, b + 2, b + 3]
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emitter(rate: f32) -> Emitter {
        Emitter {
            settings: nif::Emitter {
                speed: 2.0,
                speed_variation: 0.0,
                declination: 0.0,
                declination_variation: 0.0,
                planar_angle: 0.0,
                planar_angle_variation: 0.0,
                color: [1.0; 4],
                radius: 0.5,
                life_span: 1.0,
                life_span_variation: 0.0,
                shape: EmitterShape::Box {
                    object: None,
                    size: [0.0; 3],
                },
            },
            shape: Shape::Box {
                anchor: 0,
                size: Vec3::new(2.0, 2.0, 2.0),
            },
            keys: Some(Timed {
                controller: nif::Controller {
                    kind: ControllerKind::Emitter,
                    modifier: String::new(),
                    next: None,
                    // Active and held at the end.
                    flags: 0x0c,
                    frequency: 1.0,
                    phase: 0.0,
                    start: 0.0,
                    stop: 10.0,
                    target: None,
                    data: None,
                },
                keys: nif::EmitterKeys {
                    birth_rate: Keys {
                        interpolation: nif::Interpolation::Linear,
                        keys: vec![nif::Key {
                            time: 0.0,
                            value: rate,
                            tangents: None,
                        }],
                    },
                    active: vec![(0.0, true), (5.0, false)],
                },
            }),
            radius: None,
            speed: None,
            cycles: false,
            reset: false,
        }
    }

    fn system(steps: Vec<StepKind>) -> System {
        System {
            anchors: Vec::new(),
            world_space: true,
            capacity: 1000,
            steps: steps
                .into_iter()
                .map(|kind| Step {
                    kind,
                    active: None,
                    on: true,
                })
                .collect(),
            look: Look {
                tinted: true,
                ..default()
            },
            clock: 1.0,
            copies: Vec::new(),
        }
    }

    fn play(system: &System, seconds: f32, dt: f32) -> State {
        let space = Space {
            anchors: vec![Affine3A::IDENTITY],
        };
        let mut state = State::new(1, 0.0);
        let mut t = 0.0;
        while t < seconds - 1e-4 {
            system.step(&mut state, &space, dt);
            t += dt;
        }
        state
    }

    #[test]
    fn births_follow_the_rate_but_not_on_the_first_frame() {
        let s = system(vec![StepKind::Emit(Box::new(emitter(10.0)))]);
        assert!(play(&s, 0.1, 0.1).particles.is_empty());
        // Ten a second after the first frame, at whatever frame rate.
        assert_eq!(play(&s, 1.1, 0.1).particles.len(), 10);
        assert_eq!(play(&s, 1.1, 0.01).particles.len(), 10);
    }

    #[test]
    fn births_stop_with_the_emitter_and_at_capacity() {
        let mut s = system(vec![StepKind::Emit(Box::new(emitter(10.0)))]);
        assert_eq!(play(&s, 8.0, 0.1).particles.len(), 49);
        s.capacity = 7;
        assert_eq!(play(&s, 2.0, 0.1).particles.len(), 7);
    }

    #[test]
    fn particles_start_in_the_box_and_fly_up_its_axis() {
        let s = system(vec![StepKind::Emit(Box::new(emitter(100.0)))]);
        let state = play(&s, 0.5, 0.05);
        for p in &state.particles {
            // Moved on by its age at birth, along the model's z: Bevy's y.
            assert!(p.position.x.abs() <= 1.0 && p.position.z.abs() <= 1.0);
            assert!((p.velocity - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-4);
            assert!(p.position.y >= -1.0 && p.position.y <= 1.0 + 2.0 * 0.05 + 1e-4);
        }
    }

    #[test]
    fn particles_die_after_their_lifespan() {
        let s = system(vec![
            StepKind::Age,
            StepKind::Emit(Box::new(emitter(10.0))),
            StepKind::Move,
        ]);
        let state = play(&s, 3.0, 0.1);
        // Born over the last second only.
        assert!((9..=11).contains(&state.particles.len()));
        assert!(state.particles.iter().all(|p| p.age <= p.life));
    }

    #[test]
    fn particles_grow_in_and_fade_out() {
        let g = nif::GrowFade {
            grow: 0.5,
            grow_generation: 0,
            fade: 0.25,
            fade_generation: 0,
        };
        let p = |age| Particle {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            age,
            life: 2.0,
            radius: 1.0,
            size: 1.0,
            colour: [1.0; 4],
            generation: 0,
            id: 0,
            axis: None,
            angle: 0.0,
            new: false,
            left: None,
        };
        assert_eq!(grow_fade(&g, &p(0.0)), 0.0001);
        assert_eq!(grow_fade(&g, &p(0.25)), 0.5);
        assert_eq!(grow_fade(&g, &p(1.0)), 1.0);
        assert_eq!(grow_fade(&g, &p(1.875)), 0.5);
        let other = nif::GrowFade {
            grow_generation: 1,
            ..g
        };
        assert_eq!(grow_fade(&other, &p(1.0)), 1.0);
        assert_eq!(grow_fade(&other, &p(0.25)), 1.0);
    }

    #[test]
    fn drag_slows_only_along_its_axis() {
        let drag = StepKind::Drag {
            anchor: 0,
            axis: Vec3::Y,
            percentage: 0.5,
            range: f32::MAX,
            falloff: f32::MAX,
        };
        let s = system(vec![drag]);
        let space = Space {
            anchors: vec![Affine3A::IDENTITY],
        };
        let mut state = State::new(1, 0.0);
        state.particles.push(Particle {
            position: Vec3::ZERO,
            velocity: Vec3::new(1.0, 2.0, 0.0),
            age: 0.0,
            life: 10.0,
            radius: 1.0,
            size: 1.0,
            colour: [1.0; 4],
            generation: 0,
            id: 0,
            axis: None,
            angle: 0.0,
            new: false,
            left: None,
        });
        s.step(&mut state, &space, 0.01);
        let v = state.particles[0].velocity;
        assert_eq!(v.x, 1.0);
        assert!((v.y - 2.0 * (1.0 - 30.0 * 0.5 * 0.01)).abs() < 1e-5);
    }

    #[test]
    fn face_surface_spots_land_inside_the_triangle() {
        let mesh = EmitterMesh {
            anchor: 0,
            positions: vec![Vec3::ZERO, Vec3::X, Vec3::Z],
            normals: vec![Vec3::Y, Vec3::Y, Vec3::X],
            triangles: vec![[0, 1, 2]],
        };
        let mut rng = Rng(7);
        for _ in 0..100 {
            let (at, normal) = mesh.spot(3, &mut rng);
            assert!(at.x >= 0.0 && at.z >= 0.0 && at.x + at.z <= 1.0 + 1e-5);
            assert_eq!(normal, Some((Vec3::Y * 2.0 + Vec3::X).normalize()));
        }
        let (centre, _) = mesh.spot(1, &mut rng);
        assert!((centre - Vec3::new(1.0, 0.0, 1.0) / 3.0).length() < 1e-5);
    }

    #[test]
    fn a_modifier_counts_as_on_if_switched_on_since_the_last_frame() {
        let keys: BoolKeys = vec![(0.0, false), (1.0, true), (1.05, false)];
        assert_eq!(on_since(&keys, 0.5, 0.9, true), Some(false));
        assert_eq!(on_since(&keys, 0.9, 1.1, true), Some(true));
        assert_eq!(on_since(&keys, 1.1, 1.2, true), Some(false));
        // Looped round from near the end back to the start.
        let keys: BoolKeys = vec![(0.0, true), (0.5, false)];
        assert_eq!(on_since(&keys, 3.9, 0.1, true), Some(true));
        // Turning back, it passes only the keys it plays back over.
        assert_eq!(on_since(&keys, 1.9, 1.8, false), Some(false));
        let keys: BoolKeys = vec![(0.0, false), (1.0, true), (1.05, false)];
        assert_eq!(on_since(&keys, 1.2, 0.9, false), Some(true));
    }

    fn moving(position: Vec3, velocity: Vec3) -> Particle {
        Particle {
            position,
            velocity,
            age: 0.0,
            life: 10.0,
            radius: 1.0,
            size: 1.0,
            colour: [1.0; 4],
            generation: 0,
            id: 0,
            axis: None,
            angle: 0.0,
            new: false,
            left: None,
        }
    }

    #[test]
    fn particles_bounce_off_a_plane_within_its_extent() {
        // The ground plane, facing up: x along x, y along the model's y.
        let floor = Collider {
            anchor: 0,
            bounce: 0.5,
            die: false,
            shape: ColliderShape::Plane {
                x_axis: world_vec([1.0, 0.0, 0.0]),
                y_axis: world_vec([0.0, 1.0, 0.0]),
                width: 10.0,
                height: 10.0,
            },
        };
        let s = system(vec![StepKind::Collide(vec![floor]), StepKind::Move]);
        let space = Space {
            anchors: vec![Affine3A::IDENTITY],
        };
        let mut state = State::new(1, 0.0);
        state
            .particles
            .push(moving(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -4.0, 0.0)));
        // Off the edge it falls straight through.
        state
            .particles
            .push(moving(Vec3::new(9.0, 1.0, 0.0), Vec3::new(0.0, -4.0, 0.0)));
        s.step(&mut state, &space, 0.5);
        let [bounced, missed] = [&state.particles[0], &state.particles[1]];
        // Hit halfway through, then up at half speed for the quarter second
        // left.
        assert_eq!(bounced.velocity, Vec3::new(0.0, 2.0, 0.0));
        assert!((bounced.position.y - 2.0 * 0.25).abs() < 1e-5);
        assert!((missed.position.y + 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_sphere_keeps_particles_inside_it() {
        let ball = Collider {
            anchor: 0,
            bounce: 1.0,
            die: false,
            shape: ColliderShape::Sphere { radius: 1.0 },
        };
        let s = system(vec![StepKind::Collide(vec![ball]), StepKind::Move]);
        let space = Space {
            anchors: vec![Affine3A::IDENTITY],
        };
        let mut state = State::new(1, 0.0);
        state
            .particles
            .push(moving(Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0)));
        s.step(&mut state, &space, 0.5);
        let p = &state.particles[0];
        assert_eq!(p.velocity, Vec3::new(-4.0, 0.0, 0.0));
        assert!((p.position.x - 0.0).abs() < 1e-5);
    }

    #[test]
    fn a_collider_can_kill_what_hits_it() {
        let ball = Collider {
            anchor: 0,
            bounce: 1.0,
            die: true,
            shape: ColliderShape::Sphere { radius: 1.0 },
        };
        let s = system(vec![StepKind::Collide(vec![ball])]);
        let space = Space {
            anchors: vec![Affine3A::IDENTITY],
        };
        let mut state = State::new(1, 0.0);
        state
            .particles
            .push(moving(Vec3::new(2.0, 0.0, 0.0), Vec3::new(-4.0, 0.0, 0.0)));
        s.step(&mut state, &space, 0.5);
        assert!(state.particles.is_empty());
    }

    #[test]
    fn old_emitters_switch_by_their_emit_times() {
        // On throughout, as in both stock models.
        assert_eq!(
            old_switching(0.0, 5.0, 0.0, 6.67),
            [(0.0, true), (5.0, false)]
        );
        assert_eq!(
            old_switching(0.0, 33.3, -16.7, 33.3),
            [(0.0, true), (33.3, false)]
        );
        assert_eq!(
            old_switching(0.0, 2.0, 0.25, 0.5),
            [(0.0, false), (0.25, true), (0.5, false), (2.0, false)]
        );
        assert_eq!(old_switching(0.0, 2.0, 2.0, 3.0), [(0.0, false)]);
        assert_eq!(old_switching(0.0, 2.0, -5.0, -1.0), [(0.0, false)]);
        assert_eq!(old_switching(0.0, 2.0, 1.0, 0.5), [(0.0, false)]);
    }

    #[test]
    fn old_births_start_over_each_cycle() {
        let mut e = emitter(3.75);
        e.cycles = true;
        if let Some(k) = &mut e.keys {
            // Looping over five seconds, on throughout.
            k.controller.flags = 0x08;
            k.controller.stop = 5.0;
            k.keys.active = vec![(0.0, true), (5.0, false)];
        }
        let mut s = system(vec![StepKind::Emit(Box::new(e))]);
        // Eighteen in the first cycle, the last at 4.8 s; the nineteenth
        // would be due at 5.07 s, but the second cycle starts its count
        // over, so its first is at 5.27 s.
        assert_eq!(play(&s, 5.2, 0.05).born, 18);
        assert_eq!(play(&s, 5.3, 0.05).born, 19);
        if let StepKind::Emit(e) = &mut s.steps[0].kind {
            e.cycles = false;
        }
        assert_eq!(play(&s, 5.2, 0.05).born, 19);
    }

    #[test]
    fn cycles_wrap_however_long_the_frame() {
        let mut e = emitter(3.75);
        if let Some(k) = &mut e.keys {
            k.controller.flags = 0x08;
            k.controller.stop = 5.0;
            assert!(k.wraps(4.9, 5.1));
            // A frame of a whole cycle lands at the same key time.
            assert!(k.wraps(1.0, 6.0));
            assert!(!k.wraps(1.0, 4.0));
            // Held at the end, it never wraps.
            k.controller.flags = 0x0c;
            assert!(!k.wraps(1.0, 6.0));
        }
    }

    #[test]
    fn systems_draw_far_to_near_by_the_middle_of_their_box() {
        let view = Eye {
            at: Vec3::ZERO,
            forward: Vec3::NEG_Z,
            right: Vec3::X,
            up: Vec3::Y,
        };
        let square = |z: f32| Square {
            at: Vec3::new(0.0, 0.0, z),
            half: 1.0,
            colour: [1.0; 4],
            look: [0.0; 2],
            extra: [0.0; 4],
        };
        // Mostly near, but reaching far: the middle of its box counts, not
        // of its squares.
        let spread = [square(-1.0), square(-1.0), square(-1.0), square(-9.0)];
        assert_eq!(depth(&spread, &view), 5.0);
        let at = |depth: f32| Drawn {
            draw: 0,
            depth,
            squares: 0..1,
        };
        let mut systems = vec![at(2.0), at(5.0), at(3.0)];
        far_to_near(&mut systems);
        let order: Vec<f32> = systems.iter().map(|s| s.depth).collect();
        assert_eq!(order, [5.0, 3.0, 2.0]);
    }

    #[test]
    fn textures_are_resampled_as_they_would_be_sampled() {
        // Black and white columns, two texels wide.
        let data = [[0, 0, 0, 255], [255; 4]].repeat(2).concat();
        let image = Image::new(
            bevy::render::render_resource::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        let layer = resampled(image).unwrap();
        assert_eq!((layer.width(), layer.height()), (SIDE as u32, SIDE as u32));
        let red = |x: usize| layer.data.as_ref().unwrap()[x * 4];
        // Each column's centre keeps its colour, blending towards the other
        // between them and, as the texture repeats, across its edges.
        assert!(red(SIDE / 4) < 5 && red(SIDE * 3 / 4) > 250);
        assert!((100..156).contains(&red(SIDE / 2)));
        assert!((100..156).contains(&red(0)));
    }

    #[test]
    fn spun_particles_turn_from_their_birth() {
        let spin = StepKind::Spin {
            random: false,
            axis: Vec3::Y,
            speed: 2.0,
        };
        let s = system(vec![spin]);
        let space = Space {
            anchors: vec![Affine3A::IDENTITY],
        };
        let mut state = State::new(1, 0.0);
        let mut p = moving(Vec3::ZERO, Vec3::ZERO);
        p.new = true;
        p.age = 0.25;
        state.particles.push(p);
        s.step(&mut state, &space, 0.5);
        // Its age at birth is cleared with the frame's new flags, so it
        // turns for the whole frame.
        assert_eq!(state.particles[0].axis, Some(Vec3::Y));
        assert!((state.particles[0].angle - 1.0).abs() < 1e-5);
        s.step(&mut state, &space, 3.0);
        assert!((state.particles[0].angle - (7.0 - TAU)).abs() < 1e-4);

        let mut rng = Rng(3);
        for _ in 0..50 {
            assert!((random_axis(&mut rng).length() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn mesh_box_centres_on_camera() {
        let view = Eye {
            at: Vec3::new(10.0, 50.0, 10.0),
            forward: Vec3::NEG_Y,
            right: Vec3::X,
            up: Vec3::NEG_Z,
        };
        let square = |at| Square {
            at,
            half: 1.0,
            colour: [1.0; 4],
            look: [0.0; 2],
            extra: [0.0; 4],
        };
        let squares = [
            square(Vec3::new(30.0, 0.0, 5.0)),
            square(Vec3::new(12.0, 2.0, 40.0)),
        ];
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new())
        .with_inserted_indices(Indices::U32(Vec::new()));
        fill(&mut mesh, &mut squares.iter(), &view);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let (low, high) = positions.iter().fold((Vec3::MAX, Vec3::MIN), |(l, h), &p| {
            (l.min(p.into()), h.max(p.into()))
        });
        assert_eq!((low + high) / 2.0, Vec3::ZERO);
        let colours = mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap().len();
        assert_eq!(colours, positions.len());
        // The padding isn't drawn.
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!()
        };
        assert!(indices.iter().all(|&i| (i as usize) < squares.len() * 4));

        // With no squares, every attribute still matches the positions.
        fill(&mut mesh, &mut [].iter(), &view);
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().len();
        assert_eq!(
            mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap().len(),
            positions
        );
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!()
        };
        assert_eq!(indices, &[0, 1, 2]);
    }
}
