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
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use nif::{BoolKeys, ControllerKind, EmitterShape, Keys, ModifierKind, Nif, Object};

use crate::models::{bevy_transform, controllers};
use crate::terrain::world;

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
    material: Handle<StandardMaterial>,
    /// Its squares take each particle's colour, which its lighting may
    /// ignore.
    tinted: bool,
}

/// How a particle system is drawn.
pub(crate) struct Look {
    pub material: Handle<StandardMaterial>,
    pub tinted: bool,
}

/// Where one of a system's objects is in its model.
#[derive(Clone, Copy)]
struct Anchor {
    /// The nif index of the node above it, or the node itself.
    node: usize,
    /// From the node to it.
    below: nif::Transform,
    /// From the model's root to it.
    model: nif::Transform,
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

/// The scene objects of a model, by nif index.
pub(crate) struct Frames(HashMap<usize, Anchor>);

impl Frames {
    /// Every scene object under the model's roots, hidden ones included, as
    /// emitters may be placed by hidden helpers.
    pub fn new(nif: &Nif) -> Frames {
        let mut frames = Frames(HashMap::new());
        for &root in &nif.roots {
            frames.walk(nif, root, None, nif::Transform::IDENTITY);
        }
        frames
    }

    fn walk(&mut self, nif: &Nif, r: nif::Ref, parent: Option<usize>, above: nif::Transform) {
        let Some(i) = r else { return };
        let Some(av) = nif.get(r).and_then(Object::scene) else {
            return;
        };
        if self.0.contains_key(&i) || self.0.len() > 4096 {
            return;
        }
        let model = above.then(&av.transform);
        let anchor = match (nif.get(r), parent) {
            (Some(Object::Node(_)), _) => Anchor {
                node: i,
                below: nif::Transform::IDENTITY,
                model,
            },
            (_, Some(node)) => Anchor {
                node,
                below: av.transform,
                model,
            },
            _ => return,
        };
        self.0.insert(i, anchor);
        if let Some(Object::Node(node)) = nif.get(r) {
            for &c in &node.children {
                self.walk(nif, c, Some(i), model);
            }
        }
    }
}

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
                            anchor: anchor(*object)?,
                            size: Vec3::from(*size),
                        },
                        EmitterShape::Cylinder {
                            object,
                            radius,
                            height,
                        } => Shape::Cylinder {
                            anchor: anchor(*object)?,
                            radius: *radius,
                            height: *height,
                        },
                        EmitterShape::Sphere { object, radius } => Shape::Sphere {
                            anchor: anchor(*object)?,
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
        steps
            .iter()
            .any(|s| matches!(s.kind, StepKind::Emit(_)))
            .then_some(System {
                anchors,
                world_space: system.world_space,
                capacity,
                steps,
                material: look.material,
                tinted: look.tinted,
            })
    }
}

/// A vector in the model's z-up axes, in Bevy's.
fn world_vec([x, y, z]: [f32; 3]) -> Vec3 {
    world(x, y, z)
}

/// Spawns a model's particle systems: their anchors under `root`, or under
/// the entity `node` gives for a nif node where a rig has one.
pub(crate) fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    systems: &[Arc<System>],
    root: Entity,
    node: impl Fn(usize) -> Option<Entity>,
    seed: u32,
) {
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
        // Bevy can't upload a mesh without vertices, so it starts with an
        // empty triangle, hidden until there are particles.
        let mesh = meshes.add(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3])
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 3])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; 3])
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 3])
            .with_inserted_indices(Indices::U32(vec![0, 1, 2])),
        );
        let draw = commands
            .spawn((
                Name::new("particles"),
                Mesh3d(mesh.clone()),
                MeshMaterial3d(system.material.clone()),
                Transform::default(),
                Visibility::Hidden,
                // Its bounds change every frame.
                NoFrustumCulling,
                NotShadowCaster,
                ChildOf(anchors[0]),
            ))
            .id();
        commands.entity(anchors[0]).insert(Particles {
            system: system.clone(),
            anchors,
            draw,
            mesh,
            state: State::new(seed.wrapping_mul(31).wrapping_add(i as u32)),
        });
    }
}

/// A particle system playing.
#[derive(Component)]
pub struct Particles {
    system: Arc<System>,
    anchors: Vec<Entity>,
    draw: Entity,
    mesh: Handle<Mesh>,
    state: State,
}

/// What changes as a system plays.
struct State {
    /// Seconds it has played.
    time: f32,
    /// Particles owed to the birth rate since the emitter came on.
    owed: f32,
    played: bool,
    particles: Vec<Particle>,
    rng: Rng,
}

impl State {
    fn new(seed: u32) -> State {
        State {
            time: 0.0,
            owed: 0.0,
            played: false,
            particles: Vec::new(),
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
                (Some(a), _) => on_since(&a.keys, a.time(before), a.time(t)).unwrap_or(step.on),
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
            p.position += p.velocity * age;
            state.particles.push(p);
        }
    }
}

/// Whether on/off keys are on at `t`, or turned on at any point since
/// `before`, allowing for keys that looped round in between.
fn on_since(keys: &BoolKeys, before: f32, t: f32) -> Option<bool> {
    let passed = |at: f32| match before <= t {
        true => at > before && at <= t,
        false => at > before || at <= t,
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
        new: true,
        left: None,
    }
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

/// Plays every particle system and redraws its squares to face the camera.
pub fn play(
    time: Res<Time>,
    mut systems: Query<(&mut Particles, &GlobalTransform)>,
    anchors: Query<&GlobalTransform>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut draws: Query<(&mut Transform, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let dt = time.delta_secs();
    let Some((_, camera)) = cameras.iter().find(|(c, _)| c.is_active) else {
        return;
    };
    let (right, up) = (camera.right().as_vec3(), camera.up().as_vec3());
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
        if dt > 0.0 {
            particles.system.step(&mut particles.state, &space, dt);
        }
        let to_world = to_space.inverse();
        let scale = own.matrix3.x_axis.length();
        let from_world = own.inverse();
        let points: Vec<Vec3> = particles
            .state
            .particles
            .iter()
            .map(|p| to_world.transform_point3(p.position))
            .collect();
        // Placed at the middle of its particles, which is what systems sort
        // back to front by.
        let centre = match points.len() {
            0 => own.translation.into(),
            n => points.iter().sum::<Vec3>() / n as f32,
        };
        let local_centre = from_world.transform_point3(centre);
        if let Ok((mut t, mut visibility)) = draws.get_mut(particles.draw) {
            if t.translation != local_centre {
                t.translation = local_centre;
            }
            let shown = match points.is_empty() {
                true => Visibility::Hidden,
                false => Visibility::Inherited,
            };
            visibility.set_if_neq(shown);
            if points.is_empty() {
                continue;
            }
        }
        let Some(mut mesh) = meshes.get_mut(&particles.mesh) else {
            continue;
        };
        let n = points.len();
        let (mut positions, mut colours) = (Vec::with_capacity(n * 4), Vec::with_capacity(n * 4));
        let (mut uvs, mut normals) = (Vec::with_capacity(n * 4), Vec::with_capacity(n * 4));
        let mut indices = Vec::with_capacity(n * 6);
        let facing = from_world
            .transform_vector3(camera.back().as_vec3())
            .normalize_or_zero();
        for (p, w) in particles.state.particles.iter().zip(&points) {
            let half = p.radius * p.size * scale;
            let base = positions.len() as u32;
            for (x, y, uv) in [
                (-1.0, -1.0, [0.0, 1.0]),
                (1.0, -1.0, [1.0, 1.0]),
                (1.0, 1.0, [1.0, 0.0]),
                (-1.0, 1.0, [0.0, 0.0]),
            ] {
                let corner = *w + (right * x + up * y) * half;
                positions.push((from_world.transform_point3(corner) - local_centre).to_array());
                colours.push(match particles.system.tinted {
                    true => p.colour,
                    false => [1.0; 4],
                });
                uvs.push(uv);
                normals.push(facing.to_array());
            }
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
        mesh.insert_indices(Indices::U32(indices));
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
            material: Handle::default(),
            tinted: true,
        }
    }

    fn play(system: &System, seconds: f32, dt: f32) -> State {
        let space = Space {
            anchors: vec![Affine3A::IDENTITY],
        };
        let mut state = State::new(1);
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
        let mut state = State::new(1);
        state.particles.push(Particle {
            position: Vec3::ZERO,
            velocity: Vec3::new(1.0, 2.0, 0.0),
            age: 0.0,
            life: 10.0,
            radius: 1.0,
            size: 1.0,
            colour: [1.0; 4],
            generation: 0,
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
        assert_eq!(on_since(&keys, 0.5, 0.9), Some(false));
        assert_eq!(on_since(&keys, 0.9, 1.1), Some(true));
        assert_eq!(on_since(&keys, 1.1, 1.2), Some(false));
        // Looped round from near the end back to the start.
        let keys: BoolKeys = vec![(0.0, true), (0.5, false)];
        assert_eq!(on_since(&keys, 3.9, 0.1), Some(true));
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
        let mut state = State::new(1);
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
        let mut state = State::new(1);
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
        let mut state = State::new(1);
        state
            .particles
            .push(moving(Vec3::new(2.0, 0.0, 0.0), Vec3::new(-4.0, 0.0, 0.0)));
        s.step(&mut state, &space, 0.5);
        assert!(state.particles.is_empty());
    }
}
