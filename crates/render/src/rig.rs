//! Models whose nodes are spawned as entities, so that a `.kf` animation can
//! move them and skinned meshes bend with them: units, playing their idle
//! animation, or their moving one while they move, and once through an
//! attack or their death when told to.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Mesh, VertexAttributeValues};
use bevy::prelude::*;
use nif::{Controller, ControllerKind, Keyframes, Keys, Nif, Object};

use crate::models::{Animated, Part, PartMaterial, bevy_transform, controllers};
use crate::particles::{Anchor, Frames};
use crate::terrain::world;

/// A model's node tree, its meshes and the animations it plays.
pub struct Rig {
    /// Parents come before their children.
    nodes: Vec<RigNode>,
    /// Node indices by nif object index.
    index: HashMap<usize, usize>,
    meshes: Vec<RigMesh>,
    idle: Clip,
    /// Timed per world unit moved rather than per second.
    moving: Option<Clip>,
    /// What it plays once to use an ability, by the animation's ids.
    abilities: Vec<(String, Clip)>,
    /// Ways it can die, each held at its end.
    deaths: Vec<Clip>,
}

struct RigNode {
    /// Its nif object index.
    nif: usize,
    name: String,
    parent: Option<usize>,
    rest: nif::Transform,
}

struct RigMesh {
    part: Part,
    /// The node a rigid mesh is built relative to.
    node: Option<usize>,
    skin: Option<Skin>,
    /// Where a skinned mesh sits as modelled. Skinning places the vertices
    /// by their bones instead, so this only matters for culling.
    transform: Transform,
}

/// A mesh's hold on the nodes that bend it.
struct Skin {
    /// The node index of each of its bones.
    bones: Vec<usize>,
    poses: Handle<SkinnedMeshInverseBindposes>,
    /// Set where the mesh isn't drawn at its bones.
    apart: Option<Apart>,
}

/// How a skinned mesh is carried away from its bones: what its bones make
/// of it is measured from its skeleton's root, moved by the skin's own
/// transform, and then placed where the mesh itself is in the model.
struct Apart {
    /// The node index of the skeleton's root.
    root: usize,
    /// The node the mesh hangs from.
    node: usize,
    /// The mesh's own transform under that node, after the skin's.
    below: Affine3A,
}

/// The transform left between a skinned mesh and its bones as modelled,
/// from where the mesh and its skeleton's root are in the model and the
/// skin's own transform; `None` where they cancel out, as in most skins.
fn left_over(
    shape: &nif::Transform,
    skin: &nif::Transform,
    root: &nif::Transform,
) -> Option<nif::Transform> {
    let left = shape.then(skin).then(&root.inverse());
    let moved = [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        .into_iter()
        .any(|p| {
            let q = left.apply(p);
            (0..3).any(|i| (q[i] - p[i]).abs() > 1e-3)
        });
    moved.then_some(left)
}

/// A model-space transform as a Bevy matrix.
fn affine(t: &nif::Transform) -> Affine3A {
    Affine3A::from_mat4(bevy_transform(t).to_matrix())
}

/// One `.kf` sequence, bound to a rig's nodes by name.
struct Clip {
    channels: Vec<Channel>,
    /// Key time per second of playing.
    speed: f32,
    /// Seconds one play takes.
    length: f32,
}

struct Channel {
    node: usize,
    controller: Controller,
    keys: Moves,
    /// The translation the accumulation root is held at, so the animation's
    /// own travel doesn't carry the model away from where it is placed.
    hold: Option<[f32; 3]>,
}

/// What a channel plays on its node.
enum Moves {
    Keys(Keyframes),
    Path(nif::Path),
}

impl Moves {
    fn apply(&self, rest: &nif::Transform, time: f32) -> nif::Transform {
        match self {
            Moves::Keys(keys) => keys.apply(rest, time),
            Moves::Path(path) => path.apply(rest, time),
        }
    }
}

/// A rig's nodes as nif indices and the meshes hanging from them.
pub struct Nodes {
    nodes: Vec<RigNode>,
    /// By nif object index.
    index: HashMap<usize, usize>,
    pub shapes: Vec<RigShape>,
    /// Where every scene object is, hidden ones included, as particle
    /// emitters may be placed by hidden helpers.
    pub frames: Frames,
}

/// A visible shape among a rig's nodes.
pub struct RigShape {
    /// Its nif index.
    pub shape: usize,
    /// The rig node above it.
    pub node: usize,
    /// From the shape to the model.
    pub transform: nif::Transform,
    /// The properties in effect for it, nearest first.
    pub properties: Vec<nif::Ref>,
}

impl Nodes {
    /// The scene tree under the model's roots.
    pub fn new(nif: &Nif) -> Nodes {
        let mut nodes = Nodes {
            nodes: Vec::new(),
            index: HashMap::new(),
            shapes: Vec::new(),
            frames: Frames::default(),
        };
        for &root in &nif.roots {
            nodes.walk(nif, root, None, nif::Transform::IDENTITY, &[], false);
        }
        nodes
    }

    fn walk(
        &mut self,
        nif: &Nif,
        r: nif::Ref,
        parent: Option<usize>,
        above: nif::Transform,
        inherited: &[nif::Ref],
        hidden: bool,
    ) {
        let (Some(i), Some(object)) = (r, nif.get(r)) else {
            return;
        };
        // Nodes are never their own ancestors in the game's models, but a
        // corrupt one could make a loop.
        let Some(av) = object.scene().filter(|_| self.nodes.len() < 4096) else {
            return;
        };
        let hidden = hidden || av.hidden();
        let transform = above.then(&av.transform);
        let parent_index = parent.and_then(|p| self.nif_index(p));
        let anchor = match (object, parent_index) {
            (Object::Node(_), _) => Some(Anchor {
                node: i,
                below: nif::Transform::IDENTITY,
                model: transform,
            }),
            (_, Some(node)) => Some(Anchor {
                node,
                below: av.transform,
                model: transform,
            }),
            _ => None,
        };
        if let Some(anchor) = anchor {
            self.frames.0.entry(i).or_insert(anchor);
        }
        let properties: Vec<nif::Ref> = av.properties.iter().chain(inherited).copied().collect();
        match object {
            Object::Node(node) => {
                let at = self.nodes.len();
                self.nodes.push(RigNode {
                    nif: i,
                    name: av.name.clone(),
                    parent,
                    rest: av.transform,
                });
                self.index.insert(i, at);
                for &c in &node.children {
                    self.walk(nif, c, Some(at), transform, &properties, hidden);
                }
            }
            Object::Shape(_) if !hidden && !nif.casts_shadow_only(av) => {
                if let Some(node) = parent {
                    self.shapes.push(RigShape {
                        shape: i,
                        node,
                        transform,
                        properties,
                    });
                }
            }
            _ => {}
        }
    }

    /// Where each node is in the model, by its name; the first of a name,
    /// each node before its children, wins. As the game loads a model, its
    /// top node becomes `base`, under a new top node with no offset that
    /// takes the old one's name.
    pub fn points(&self) -> HashMap<String, nif::Transform> {
        let mut model: Vec<nif::Transform> = Vec::with_capacity(self.nodes.len());
        let mut out = HashMap::new();
        if let Some(top) = self.nodes.first() {
            out.insert(top.name.clone(), nif::Transform::IDENTITY);
            out.insert("base".to_string(), top.rest);
        }
        for (i, n) in self.nodes.iter().enumerate() {
            let t = match n.parent {
                Some(p) => model[p].then(&n.rest),
                None => n.rest,
            };
            model.push(t);
            if i > 0 {
                out.entry(n.name.clone()).or_insert(t);
            }
        }
        out
    }

    /// The nif index of rig node `node`.
    fn nif_index(&self, node: usize) -> Option<usize> {
        self.nodes.get(node).map(|n| n.nif)
    }

    /// The rig for these nodes, with parts for [`Nodes::shapes`] by index:
    /// built in its own space for a skinned shape, else in its node's.
    /// The clips are `.kf` files with how long one play takes: in world
    /// units moved for `moving`, else in seconds. With no `idle` clip it
    /// idles with the keyframes the model's own nodes carry.
    #[allow(clippy::too_many_arguments)]
    pub fn rig(
        self,
        nif: &Nif,
        meshes: Vec<(Part, usize)>,
        idle: Option<(&Nif, f32)>,
        moving: Option<(&Nif, f32)>,
        abilities: &[(String, Nif, f32)],
        deaths: &[(Nif, f32)],
        assets: &mut Assets<Mesh>,
        skins: &mut Assets<SkinnedMeshInverseBindposes>,
    ) -> Option<Rig> {
        let moving = moving.and_then(|(kf, length)| self.clip(kf, length));
        let idle = match idle {
            Some((kf, length)) => self.clip(kf, length)?,
            None => self.own_clip(nif)?,
        };
        let abilities = abilities
            .iter()
            .filter_map(|(ids, kf, length)| Some((ids.clone(), self.clip(kf, *length)?)))
            .collect();
        let deaths = deaths
            .iter()
            .filter_map(|(kf, length)| self.clip(kf, *length))
            .collect();
        let meshes = meshes
            .into_iter()
            .filter_map(|(part, i)| {
                let RigShape {
                    shape,
                    node,
                    transform: model,
                    ..
                } = *self.shapes.get(i)?;
                let skinned = nif
                    .get_as::<nif::Shape>(Some(shape))
                    .is_some_and(|s| s.skin.is_some());
                let skin = self.skin(nif, shape, node, &model, &part.mesh, assets, skins);
                // A skin that can't be bound stands as it was modelled.
                let (node, transform) = match skinned {
                    true => (None, bevy_transform(&model)),
                    false => (Some(node), Transform::IDENTITY),
                };
                Some(RigMesh {
                    part,
                    node,
                    skin,
                    transform,
                })
            })
            .collect();
        Some(Rig {
            nodes: self.nodes,
            index: self.index,
            meshes,
            idle,
            moving,
            abilities,
            deaths,
        })
    }

    /// Binds the mesh of a skinned shape under rig node `node`, at `model`
    /// in the model, to its bones: their nodes and inverse bind poses, with
    /// each vertex's four strongest weights added to the mesh.
    #[allow(clippy::too_many_arguments)]
    fn skin(
        &self,
        nif: &Nif,
        shape: usize,
        node: usize,
        model: &nif::Transform,
        mesh: &Handle<Mesh>,
        assets: &mut Assets<Mesh>,
        skins: &mut Assets<SkinnedMeshInverseBindposes>,
    ) -> Option<Skin> {
        let shape = nif.get_as::<nif::Shape>(Some(shape))?;
        let instance = nif.get_as::<nif::SkinInstance>(shape.skin)?;
        let data = nif.get_as::<nif::SkinData>(instance.data)?;
        let joints = instance
            .bones
            .iter()
            .map(|b| self.index.get(&(*b)?).copied())
            .collect::<Option<Vec<_>>>()?;
        if joints.len() != data.bones.len() || joints.len() > 256 {
            return None;
        }
        let mut mesh = assets.get_mut(mesh)?;
        let n = mesh.count_vertices();
        let mut influences = vec![Vec::new(); n];
        for (j, bone) in data.bones.iter().enumerate() {
            for &(v, w) in &bone.weights {
                if let Some(list) = influences.get_mut(v as usize) {
                    list.push((j as u16, w));
                }
            }
        }
        let (indices, weights): (Vec<[u16; 4]>, Vec<[f32; 4]>) = influences
            .into_iter()
            .map(|mut list| {
                list.sort_by(|a, b| b.1.total_cmp(&a.1));
                list.truncate(4);
                let total: f32 = list.iter().map(|(_, w)| w).sum();
                let (mut i, mut w) = ([0; 4], [0.0; 4]);
                for (k, (j, weight)) in list.into_iter().enumerate() {
                    i[k] = j;
                    w[k] = if total > 0.0 { weight / total } else { 0.0 };
                }
                // A vertex no bone holds follows the first.
                if total <= 0.0 {
                    w[0] = 1.0;
                }
                (i, w)
            })
            .unzip();
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(indices),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights);
        let poses: Vec<Mat4> = data
            .bones
            .iter()
            .map(|b| bevy_transform(&b.transform).to_matrix())
            .collect();
        // Skins that name a root outside the rig are drawn at their bones.
        let apart = instance.skeleton_root.and_then(|r| {
            let root = *self.index.get(&r)?;
            left_over(model, &data.transform, &self.frames.0.get(&r)?.model)?;
            Some(Apart {
                root,
                node,
                below: affine(&shape.av.transform) * affine(&data.transform),
            })
        });
        Some(Skin {
            bones: joints,
            poses: skins.add(SkinnedMeshInverseBindposes::from(poses)),
            apart,
        })
    }

    /// The keyframes and paths on the model's own nodes, looping as each
    /// controller says; a node with several plays them in its chain's
    /// order, each on top of the last.
    fn own_clip(&self, nif: &Nif) -> Option<Clip> {
        let channels: Vec<Channel> = self
            .index
            .iter()
            .flat_map(|(&i, &node)| {
                let av = nif.get(Some(i)).and_then(Object::scene);
                let chain = av.map(|av| controllers(nif, av.controller));
                chain.into_iter().flatten().filter_map(move |c| {
                    let keys = match c.kind {
                        ControllerKind::Keyframe => {
                            Moves::Keys(nif.get_as::<Keyframes>(c.data)?.clone())
                        }
                        ControllerKind::Path(settings) => Moves::Path(nif::Path::new(
                            settings,
                            nif.get_as::<Keys<[f32; 3]>>(settings.positions)?.clone(),
                            nif.get_as::<Keys<f32>>(c.data)?.clone(),
                        )),
                        _ => return None,
                    };
                    Some(Channel {
                        node,
                        controller: c.clone(),
                        keys,
                        hold: None,
                    })
                })
            })
            .collect();
        let length = channels
            .iter()
            .map(|c| c.controller.stop - c.controller.start)
            .fold(0.0, f32::max);
        (!channels.is_empty()).then_some(Clip {
            channels,
            speed: 1.0,
            length,
        })
    }

    /// Binds a `.kf` sequence's controllers to the nodes they name, timed so
    /// that one play from its `start` to `end` text keys takes `length`
    /// seconds.
    fn clip(&self, kf: &Nif, length: f32) -> Option<Clip> {
        let sequence = kf
            .roots
            .iter()
            .find_map(|&r| kf.get_as::<nif::Sequence>(r))?;
        // The first of a name wins, as for [`Nodes::points`].
        let mut by_name: HashMap<&str, usize> = HashMap::new();
        for (i, n) in self.nodes.iter().enumerate() {
            by_name.entry(n.name.as_str()).or_insert(i);
        }
        let channels: Vec<Channel> = sequence
            .blocks
            .iter()
            .filter_map(|b| {
                let node = *by_name.get(b.target.as_str())?;
                let controller = kf.get_as::<Controller>(b.controller)?;
                if controller.kind != ControllerKind::Keyframe {
                    return None;
                }
                let keys = kf.get_as::<Keyframes>(controller.data)?;
                let hold = (b.target == sequence.accumulation_root).then(|| {
                    keys.translation
                        .sample(controller.start)
                        .unwrap_or(self.nodes[node].rest.translation)
                });
                Some(Channel {
                    node,
                    controller: controller.clone(),
                    keys: Moves::Keys(keys.clone()),
                    hold,
                })
            })
            .collect();
        if channels.is_empty() {
            return None;
        }
        let keys = kf.get_as::<nif::TextKeys>(sequence.text_keys);
        let time = |text: &str| keys?.keys.iter().find(|k| k.text == text).map(|k| k.time);
        let span = match (time("start"), time("end")) {
            (Some(start), Some(end)) if end > start => end - start,
            _ => length,
        };
        Some(Clip {
            channels,
            speed: if length > 0.0 { span / length } else { 1.0 },
            length,
        })
    }
}

impl Rig {
    /// Its meshes' parts, in the order [`Rig::spawn`] takes their materials.
    pub fn parts(&self) -> impl Iterator<Item = &Part> {
        self.meshes.iter().map(|m| &m.part)
    }

    /// The node a nif object index is, if it's one of the rig's.
    pub fn joint(&self, nif_index: usize) -> Option<usize> {
        self.index.get(&nif_index).copied()
    }

    /// Spawns its nodes and meshes under `root`, each mesh with its own of
    /// `materials`, returning the nodes' entities.
    pub fn spawn(
        self: &Arc<Rig>,
        commands: &mut Commands,
        root: Entity,
        offset: f32,
        materials: Vec<PartMaterial>,
    ) -> Vec<Entity> {
        let mut joints: Vec<Entity> = Vec::with_capacity(self.nodes.len());
        for (i, node) in self.nodes.iter().enumerate() {
            let parent = node.parent.map_or(root, |p| joints[p]);
            let transform = bevy_transform(&self.pose(i, offset));
            let id = commands
                .spawn((
                    Name::new(node.name.clone()),
                    transform,
                    Visibility::default(),
                    ChildOf(parent),
                ))
                .id();
            joints.push(id);
        }
        for (mesh, material) in self.meshes.iter().zip(materials) {
            let parent = mesh.node.map_or(root, |n| joints[n]);
            let mut child = commands.spawn((
                Mesh3d(mesh.part.mesh.clone()),
                mesh.transform,
                ChildOf(parent),
            ));
            material.insert(&mut child);
            if let Some(animation) = &mesh.part.animation {
                child.insert(Animated::new(animation.clone(), offset, material));
            }
            if mesh.part.blended {
                child.insert(NotShadowCaster);
            }
            let Some(skin) = &mesh.skin else { continue };
            let bones: Vec<Entity> = skin.bones.iter().map(|&b| joints[b]).collect();
            let Some(apart) = &skin.apart else {
                child.insert(SkinnedMesh {
                    inverse_bindposes: skin.poses.clone(),
                    joints: bones,
                });
                continue;
            };
            // The mesh follows stand-ins for its bones, which
            // [`carry_skins`] places; with no transform of their own, they
            // aren't placed by their parent.
            let child = child.id();
            let stand_ins: Vec<Entity> = bones
                .iter()
                .map(|_| {
                    commands
                        .spawn((GlobalTransform::default(), ChildOf(root)))
                        .id()
                })
                .collect();
            commands.entity(child).insert((
                SkinnedMesh {
                    inverse_bindposes: skin.poses.clone(),
                    joints: stand_ins.clone(),
                },
                Carried {
                    root: joints[apart.root],
                    node: joints[apart.node],
                    below: apart.below,
                    bones,
                    stand_ins,
                },
            ));
        }
        commands.entity(root).insert(Playing {
            rig: self.clone(),
            joints: joints.clone(),
            time: offset,
            moving: None,
            once: None,
            rate: 1.0,
        });
        joints
    }

    /// Node `i`'s transform below its parent, `t` seconds into idling.
    fn pose(&self, i: usize, t: f32) -> nif::Transform {
        let channels = self.idle.channels.iter().filter(|c| c.node == i);
        pose(channels, self.nodes[i].rest, t * self.idle.speed)
    }
}

impl Channel {
    fn pose(&self, rest: &nif::Transform, time: f32) -> nif::Transform {
        let mut t = self.keys.apply(rest, self.controller.key_time(time));
        if let Some(hold) = self.hold {
            t.translation = hold;
        }
        t
    }
}

/// A node's channels played in turn on its rest pose.
fn pose<'a>(
    channels: impl IntoIterator<Item = &'a Channel>,
    rest: nif::Transform,
    t: f32,
) -> nif::Transform {
    channels.into_iter().fold(rest, |pose, c| c.pose(&pose, t))
}

/// A skinned mesh drawn away from its bones, on the mesh.
#[derive(Component)]
pub struct Carried {
    root: Entity,
    node: Entity,
    below: Affine3A,
    bones: Vec<Entity>,
    /// What the mesh is bound to instead of each bone.
    stand_ins: Vec<Entity>,
}

/// Puts the stand-ins for each [`Carried`] mesh's bones where its bones
/// are, carried from its skeleton's root to where the mesh itself is. Runs
/// once transforms are propagated.
pub fn carry_skins(skins: Query<&Carried>, mut places: Query<&mut GlobalTransform>) {
    for skin in &skins {
        let (Ok(node), Ok(root)) = (places.get(skin.node), places.get(skin.root)) else {
            continue;
        };
        let carry = node.affine() * skin.below * root.affine().inverse();
        for (&bone, &stand_in) in skin.bones.iter().zip(&skin.stand_ins) {
            let Ok(bone) = places.get(bone).map(|b| b.affine()) else {
                continue;
            };
            if let Ok(mut place) = places.get_mut(stand_in) {
                *place = GlobalTransform::from(carry * bone);
            }
        }
    }
}

/// Squashes `transform` onto the one axis `pose` has left, where a path has
/// it travelling straight up or down; such a rotation has no Bevy rotation.
fn squash(transform: &mut Transform, pose: &nif::Transform) {
    let r = pose.rotation;
    let column = |c: usize| world(r[0][c], r[1][c], r[2][c]);
    if column(0).dot(column(1).cross(column(2))).abs() > 1e-6 {
        return;
    }
    let Some(kept) = (0..3).max_by(|&a, &b| column(a).length().total_cmp(&column(b).length()))
    else {
        return;
    };
    let along = column(kept);
    let mut local = [0.0; 3];
    local[kept] = 1.0;
    let local = world(local[0], local[1], local[2]);
    transform.rotation = Quat::from_rotation_arc(local, along.normalize_or(local));
    // Not quite zero, which would leave the node's children with no
    // inverse.
    transform.scale = (local.abs() * (along.length() - 1e-4) + Vec3::splat(1e-4)) * pose.scale;
}

/// Plays a rig's animation on the node entities spawned for it.
#[derive(Component)]
pub struct Playing {
    rig: Arc<Rig>,
    joints: Vec<Entity>,
    /// Into the current animation, in seconds, or world units moved while
    /// moving.
    time: f32,
    /// Speed in world units a second while moving.
    moving: Option<f32>,
    /// An animation it's playing once instead.
    once: Option<Once>,
    /// Seconds of animation per second played.
    pub(crate) rate: f32,
}

struct Once {
    clip: OnceClip,
    /// Seconds into it.
    time: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum OnceClip {
    /// Index into [`Rig::abilities`].
    Ability(usize),
    /// Index into [`Rig::deaths`].
    Death(usize),
}

impl Rig {
    fn once(&self, clip: OnceClip) -> &Clip {
        match clip {
            OnceClip::Ability(i) => &self.abilities[i].1,
            OnceClip::Death(i) => &self.deaths[i],
        }
    }
}

impl Playing {
    /// Plays the ability animation with these ids once from the start,
    /// unless it's dying or has none.
    pub fn use_ability(&mut self, ids: &str) {
        let Some(i) = self
            .rig
            .abilities
            .iter()
            .position(|(a, _)| a.eq_ignore_ascii_case(ids))
        else {
            return;
        };
        if !self.dying() {
            self.once = Some(Once {
                clip: OnceClip::Ability(i),
                time: 0.0,
            });
        }
    }

    /// Plays one of its deaths, picked by `pick`, and stays at its end;
    /// false if it has none to play.
    pub fn die(&mut self, pick: usize) -> bool {
        if self.rig.deaths.is_empty() {
            return false;
        }
        if !self.dying() {
            self.once = Some(Once {
                clip: OnceClip::Death(pick % self.rig.deaths.len()),
                time: 0.0,
            });
        }
        true
    }

    /// Back from its death to idling, for one that's been replaced.
    pub fn revive(&mut self) {
        if self.dying() {
            self.once = None;
            self.time = 0.0;
        }
    }

    /// Whether it's playing something once rather than idling or moving.
    pub fn busy(&self) -> bool {
        self.once.is_some()
    }

    /// Seconds one play of its idle animation takes.
    pub(crate) fn idle_length(&self) -> f32 {
        self.rig.idle.length
    }

    fn dying(&self) -> bool {
        matches!(&self.once, Some(o) if matches!(o.clip, OnceClip::Death(_)))
    }

    /// Switches between idling and moving at `speed`, for rigs with a
    /// moving animation.
    pub fn set_moving(&mut self, speed: Option<f32>) {
        let speed = speed.filter(|_| self.rig.moving.is_some());
        if speed.is_some() != self.moving.is_some() {
            self.time = 0.0;
        }
        self.moving = speed;
    }
}

pub fn play(
    time: Res<Time>,
    real: Res<Time<Real>>,
    mut rigs: Query<(&mut Playing, Has<crate::portrait::Sitting>)>,
    mut transforms: Query<&mut Transform>,
) {
    for (mut p, sitting) in &mut rigs {
        let p = &mut *p;
        // A portrait's sitter carries on while the game is paused.
        let dt = match sitting {
            true => real.delta_secs(),
            false => time.delta_secs() * p.rate,
        };
        if let Some(once) = &mut p.once {
            once.time += dt;
            let ended = once.time >= p.rig.once(once.clip).length;
            if ended && matches!(once.clip, OnceClip::Ability(_)) {
                p.once = None;
            }
        }
        let (clip, time) = match &p.once {
            Some(once) => {
                let clip = p.rig.once(once.clip);
                // Short of the end, which looping clips would wrap around.
                (clip, once.time.min(clip.length * 0.999))
            }
            None => {
                p.time += dt * p.moving.unwrap_or(1.0);
                let clip = match (&p.rig.moving, p.moving) {
                    (Some(clip), Some(_)) => clip,
                    _ => &p.rig.idle,
                };
                (clip, p.time)
            }
        };
        let t = time * clip.speed;
        // Each node's channels are next to each other.
        for channels in clip.channels.chunk_by(|a, b| a.node == b.node) {
            let node = channels[0].node;
            let pose = pose(channels, p.rig.nodes[node].rest, t);
            if let Ok(mut transform) = transforms.get_mut(p.joints[node]) {
                *transform = bevy_transform(&pose);
                if channels.iter().any(|c| matches!(c.keys, Moves::Path(_))) {
                    squash(&mut transform, &pose);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use nif::{Interpolation, Key, PathSettings, SceneObject};

    fn node(name: &str, controller: nif::Ref, children: Vec<nif::Ref>) -> Object {
        Object::Node(nif::Node {
            av: SceneObject {
                name: name.into(),
                extra_data: Vec::new(),
                controller,
                flags: 0,
                transform: nif::Transform::IDENTITY,
                properties: Vec::new(),
            },
            children,
            billboard: None,
        })
    }

    fn linear<T>(keys: &[(f32, T)]) -> Keys<T>
    where
        T: Copy,
    {
        Keys {
            interpolation: Interpolation::Linear,
            keys: keys
                .iter()
                .map(|&(time, value)| Key {
                    time,
                    value,
                    tangents: None,
                })
                .collect(),
        }
    }

    /// A streamer node moved 4 along the game's +x over its path's first
    /// second, the path followed by `axis` with `flags`, and after it in
    /// its chain, with `scale`, keyframes doubling its size.
    fn path_model(flags: u16, axis: i16, scale: bool) -> Nif {
        let timing = |next, data| Controller {
            kind: ControllerKind::Keyframe,
            modifier: String::new(),
            next,
            flags: 0x28,
            frequency: 1.0,
            phase: 0.0,
            start: 0.0,
            stop: 2.0,
            target: Some(1),
            data,
        };
        Nif {
            objects: vec![
                node("root", None, vec![Some(1)]),
                node("streamer", Some(2), Vec::new()),
                Object::Controller(Controller {
                    kind: ControllerKind::Path(PathSettings {
                        flags,
                        bank_direction: 1,
                        max_bank: 0.5,
                        smoothing: 0.02,
                        follow_axis: axis,
                        positions: Some(3),
                    }),
                    ..timing(scale.then_some(5), Some(4))
                }),
                Object::Positions(linear(&[(0.0, [0.0; 3]), (1.0, [4.0, 0.0, 0.0])])),
                Object::Floats(linear(&[(0.0, 0.0), (2.0, 1.0)])),
                Object::Controller(timing(None, Some(6))),
                Object::Keyframes(Keyframes {
                    scale: linear(&[(0.0, 2.0)]),
                    ..Default::default()
                }),
            ],
            roots: vec![Some(0)],
            incomplete: None,
        }
    }

    /// The streamer's pose `t` seconds in.
    fn streamer_pose(nif: &Nif, t: f32) -> nif::Transform {
        let nodes = Nodes::new(nif);
        let clip = nodes.own_clip(nif).expect("a path alone makes a clip");
        assert_eq!(clip.length, 2.0);
        let streamer = nodes.index[&1];
        let channels = clip.channels.iter().filter(|c| c.node == streamer);
        pose(channels, nif::Transform::IDENTITY, t)
    }

    #[test]
    fn plays_paths_on_their_own() {
        let nif = path_model(0x32, 2, false);
        assert_eq!(streamer_pose(&nif, 0.5).translation, [1.0, 0.0, 0.0]);
        let t = streamer_pose(&nif, 1.0);
        assert_eq!(t.translation, [2.0, 0.0, 0.0]);
        // Facing along +x.
        assert_eq!(t.rotation.map(|r| r[2]), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn plays_keyframes_after_paths() {
        let t = streamer_pose(&path_model(0x32, 2, true), 1.0);
        assert_eq!((t.translation, t.scale), ([2.0, 0.0, 0.0], 2.0));
    }

    #[test]
    fn turns_flipped_x_followers_in_bevy() {
        // The lily fish's flags: its -x leads, its z stays up.
        let t = bevy_transform(&streamer_pose(&path_model(0x70, 0, false), 1.0));
        assert!((t.rotation * Vec3::X).distance(Vec3::NEG_X) < 1e-4);
        assert!((t.rotation * Vec3::Y).distance(Vec3::Y) < 1e-4);
    }

    #[test]
    fn squashes_nodes_onto_their_line() {
        // Only the z axis left, along the game's +x.
        let pose = nif::Transform {
            rotation: [[0.0, 0.0, 1.0], [0.0; 3], [0.0; 3]],
            ..nif::Transform::IDENTITY
        };
        let mut t = bevy_transform(&pose);
        squash(&mut t, &pose);
        let onto = |v: Vec3| t.transform_point(v);
        // The game's z is Bevy's y, and lands on the game's x, Bevy's x.
        assert!(onto(Vec3::Y).distance(Vec3::X) < 1e-3);
        assert!(onto(Vec3::X).length() < 1e-3 && onto(Vec3::Z).length() < 1e-3);
        let turned = nif::Transform::IDENTITY;
        let mut t = bevy_transform(&turned);
        squash(&mut t, &turned);
        assert_eq!(t, bevy_transform(&turned));
    }

    fn rig_node(name: &str, parent: Option<usize>, at: [f32; 3]) -> RigNode {
        RigNode {
            nif: 0,
            name: name.into(),
            parent,
            rest: nif::Transform {
                translation: at,
                ..nif::Transform::IDENTITY
            },
        }
    }

    #[test]
    fn skins_are_apart_from_their_bones_only_by_what_is_left_over() {
        let at = |translation: [f32; 3]| nif::Transform {
            translation,
            ..nif::Transform::IDENTITY
        };
        // Bound where it is: the skin's transform undoes the mesh's place.
        let (shape, skin) = (at([1.0, 2.0, 3.0]), at([-1.0, -2.0, -3.0]));
        assert_eq!(left_over(&shape, &skin, &nif::Transform::IDENTITY), None);
        // An owl modelled 10 up, bound at its bones.
        let up = at([0.0, 0.0, 10.0]);
        let left = left_over(&up, &nif::Transform::IDENTITY, &nif::Transform::IDENTITY);
        assert_eq!(left.map(|t| t.translation), Some([0.0, 0.0, 10.0]));
        // Measured from the skeleton's root, wherever that is.
        assert_eq!(left_over(&up, &nif::Transform::IDENTITY, &up), None);
    }

    #[test]
    fn carried_skins_follow_their_bones_from_where_the_mesh_is() {
        let mut world = World::new();
        let at = |v: Vec3| GlobalTransform::from(Transform::from_translation(v));
        // The whole model stands at x = 100; the mesh's node is 10 above
        // the skeleton's root and a bone is 2 ahead of it.
        let root = world.spawn(at(Vec3::new(100.0, 0.0, 0.0))).id();
        let node = world.spawn(at(Vec3::new(100.0, 10.0, 0.0))).id();
        let bone = world.spawn(at(Vec3::new(100.0, 0.0, 2.0))).id();
        let stand_in = world.spawn(GlobalTransform::default()).id();
        world.spawn(Carried {
            root,
            node,
            below: Affine3A::IDENTITY,
            bones: vec![bone],
            stand_ins: vec![stand_in],
        });
        world.run_system_once(carry_skins).expect("runs");
        let placed = world.get::<GlobalTransform>(stand_in).expect("placed");
        assert!(
            placed
                .translation()
                .abs_diff_eq(Vec3::new(100.0, 10.0, 2.0), 1e-5)
        );
    }

    #[test]
    fn clips_move_the_first_node_of_a_name() {
        let nodes = Nodes {
            nodes: vec![
                rig_node("Scene Root", None, [0.0; 3]),
                rig_node("Gun", Some(0), [0.0; 3]),
                rig_node("Gun", Some(1), [0.0; 3]),
            ],
            index: HashMap::new(),
            shapes: Vec::new(),
            frames: Frames::default(),
        };
        let kf = Nif {
            objects: vec![
                Object::Sequence(nif::Sequence {
                    name: "idle".into(),
                    accumulation_root: String::new(),
                    text_keys: None,
                    blocks: vec![nif::ControlledBlock {
                        target: "Gun".into(),
                        controller: Some(1),
                    }],
                }),
                Object::Controller(Controller {
                    kind: ControllerKind::Keyframe,
                    modifier: String::new(),
                    next: None,
                    flags: 8,
                    frequency: 1.0,
                    phase: 0.0,
                    start: 0.0,
                    stop: 1.0,
                    target: None,
                    data: Some(2),
                }),
                Object::Keyframes(Keyframes::default()),
            ],
            roots: vec![Some(0)],
            incomplete: None,
        };
        let clip = nodes.clip(&kf, 1.0).expect("a clip");
        assert_eq!(
            clip.channels.iter().map(|c| c.node).collect::<Vec<_>>(),
            [1]
        );
    }

    #[test]
    fn points_are_found_first_from_the_top_with_the_top_as_base() {
        let nodes = Nodes {
            nodes: vec![
                rig_node("Scene Root", None, [0.0, 0.0, 1.0]),
                rig_node("Gun", Some(0), [2.0, 0.0, 0.0]),
                rig_node("base", Some(1), [0.0, 3.0, 0.0]),
                rig_node("Gun", Some(0), [9.0, 0.0, 0.0]),
            ],
            index: HashMap::new(),
            shapes: Vec::new(),
            frames: Frames::default(),
        };
        let points = nodes.points();
        let at = |name: &str| points.get(name).map(|t| t.translation);
        assert_eq!(at("Scene Root"), Some([0.0; 3]));
        assert_eq!(at("base"), Some([0.0, 0.0, 1.0]));
        assert_eq!(at("Gun"), Some([2.0, 0.0, 1.0]));
        assert_eq!(at("gun"), None);
    }
}
