//! Particle systems playing: their particles born, moved and dying.

use crate::models::{Animated, MaterialStores};
use crate::space::{bevy_transform, in_world};
use bevy::math::Affine3A;
use bevy::prelude::*;
use nif::BoolKeys;
use std::f32::consts::TAU;
use std::sync::Arc;

use super::{Collider, ColliderShape, Emitter, EmitterMesh, Shape, StepKind, System};

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

/// A particle system playing. Its squares are drawn by [`super::draw`].
#[derive(Component)]
pub struct Particles {
    pub(super) system: Arc<System>,
    anchors: Vec<Entity>,
    pub(super) state: State,
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
                let material = match part.paints() {
                    true => materials.copy(&part.material),
                    false => part.material.clone(),
                };
                let animated = part.animation.as_ref().map(|animation| {
                    Animated::at_rate(animation.clone(), state.time, rate, material.clone())
                });
                let posed = animated.as_ref().and_then(|a| a.pose(&mut materials));
                let t = posed.unwrap_or_default();
                let mut child = commands.spawn((t, global * t, ChildOf(copy)));
                part.draw(&mut child, &material);
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
pub(super) struct State {
    /// Seconds into its model's animation, which its keys are read at.
    time: f32,
    /// Particles owed to the birth rate since the emitter came on.
    owed: f32,
    played: bool,
    pub(super) particles: Vec<Particle>,
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
pub(super) struct Particle {
    /// In world space for a world-space system, else the system's own.
    pub(super) position: Vec3,
    velocity: Vec3,
    age: f32,
    life: f32,
    pub(super) radius: f32,
    pub(super) size: f32,
    pub(super) colour: [f32; 4],
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
    let direction = in_world([
        declination.sin() * planar.cos(),
        declination.sin() * planar.sin(),
        declination.cos(),
    ]);
    let life = e.life_span + rng.signed() * e.life_span_variation / 2.0;
    let (position, velocity) = match shape {
        Shape::Box { anchor, size } => {
            let at = Vec3::new(rng.signed(), rng.signed(), rng.signed())
                * in_world(size.to_array()).abs()
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
            let at = in_world([r * angle.cos(), r * angle.sin(), h]);
            placed(space.anchors[*anchor], at, direction * speed)
        }
        Shape::Sphere { anchor, radius } => {
            let (r, a, b) = (rng.unit() * radius, rng.unit() * TAU, rng.unit() * TAU);
            let at = in_world([r * a.cos() * b.sin(), r * a.sin() * b.sin(), r * b.cos()]);
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
    in_world([r * about.cos(), r * about.sin(), z])
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

#[cfg(test)]
mod tests {
    use super::super::{Look, Step, Timed, old_switching};
    use super::*;
    use nif::{ControllerKind, EmitterShape, Keys};

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
                x_axis: in_world([1.0, 0.0, 0.0]),
                y_axis: in_world([0.0, 1.0, 0.0]),
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
}
