//! Models whose nodes are spawned as entities, so that a `.kf` animation can
//! move them and skinned meshes bend with them: units, playing their idle
//! animation, or their moving one while they move, and once through an
//! attack or their death when told to.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::light::NotShadowCaster;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Mesh, VertexAttributeValues};
use bevy::prelude::*;
use nif::{Controller, ControllerKind, Keyframes, Nif, Object};

use crate::models::{Part, PartMaterial, bevy_transform};

/// A model's node tree, its meshes and the animations it plays.
pub struct Rig {
    /// Parents come before their children.
    nodes: Vec<RigNode>,
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
    name: String,
    parent: Option<usize>,
    rest: nif::Transform,
}

struct RigMesh {
    part: Part,
    /// The node a rigid mesh is built relative to.
    node: Option<usize>,
    /// The node index of each of the skin's bones.
    skin: Option<(Vec<usize>, Handle<SkinnedMeshInverseBindposes>)>,
    /// Where a skinned mesh sits as modelled. Skinning places the vertices
    /// by their bones instead, so this only matters for culling.
    transform: Transform,
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
    keys: Keyframes,
    /// The translation the accumulation root is held at, so the animation's
    /// own travel doesn't carry the model away from where it is placed.
    hold: Option<[f32; 3]>,
}

/// A rig's nodes as nif indices and the meshes hanging from them.
pub struct Nodes {
    nodes: Vec<RigNode>,
    /// By nif object index.
    index: HashMap<usize, usize>,
    pub shapes: Vec<RigShape>,
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
        let properties: Vec<nif::Ref> = av.properties.iter().chain(inherited).copied().collect();
        match object {
            Object::Node(node) => {
                let at = self.nodes.len();
                self.nodes.push(RigNode {
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

    /// The rig for these nodes, with parts for [`Nodes::shapes`] by index:
    /// built in its own space for a skinned shape, else in its node's.
    /// The clips are `.kf` files with how long one play takes: in world
    /// units moved for `moving`, else in seconds.
    #[allow(clippy::too_many_arguments)]
    pub fn rig(
        self,
        nif: &Nif,
        meshes: Vec<(Part, usize)>,
        idle: (&Nif, f32),
        moving: Option<(&Nif, f32)>,
        abilities: &[(String, Nif, f32)],
        deaths: &[(Nif, f32)],
        assets: &mut Assets<Mesh>,
        skins: &mut Assets<SkinnedMeshInverseBindposes>,
    ) -> Option<Rig> {
        let moving = moving.and_then(|(kf, length)| self.clip(kf, length));
        let idle = self.clip(idle.0, idle.1)?;
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
                let skin = self.skin(nif, shape, &part.mesh, assets, skins);
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
            meshes,
            idle,
            moving,
            abilities,
            deaths,
        })
    }

    /// Binds the mesh of a skinned shape to its bones: their nodes and
    /// inverse bind poses, with each vertex's four strongest weights added
    /// to the mesh.
    fn skin(
        &self,
        nif: &Nif,
        shape: usize,
        mesh: &Handle<Mesh>,
        assets: &mut Assets<Mesh>,
        skins: &mut Assets<SkinnedMeshInverseBindposes>,
    ) -> Option<(Vec<usize>, Handle<SkinnedMeshInverseBindposes>)> {
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
        Some((joints, skins.add(SkinnedMeshInverseBindposes::from(poses))))
    }

    /// Binds a `.kf` sequence's controllers to the nodes they name, timed so
    /// that one play from its `start` to `end` text keys takes `length`
    /// seconds.
    fn clip(&self, kf: &Nif, length: f32) -> Option<Clip> {
        let sequence = kf
            .roots
            .iter()
            .find_map(|&r| kf.get_as::<nif::Sequence>(r))?;
        let by_name: HashMap<&str, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.name.as_str(), i))
            .collect();
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
                    keys: keys.clone(),
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

    /// Spawns its nodes and meshes under `root`, each mesh with its own of
    /// `materials`.
    pub fn spawn(
        self: &Arc<Rig>,
        commands: &mut Commands,
        root: Entity,
        offset: f32,
        materials: Vec<PartMaterial>,
    ) {
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
            if mesh.part.blended {
                child.insert(NotShadowCaster);
            }
            if let Some((bones, poses)) = &mesh.skin {
                child.insert(SkinnedMesh {
                    inverse_bindposes: poses.clone(),
                    joints: bones.iter().map(|&b| joints[b]).collect(),
                });
            }
        }
        commands.entity(root).insert(Playing {
            rig: self.clone(),
            joints,
            time: offset,
            moving: None,
            once: None,
        });
    }

    /// Node `i`'s transform below its parent, `t` seconds into idling.
    fn pose(&self, i: usize, t: f32) -> nif::Transform {
        let rest = self.nodes[i].rest;
        let Some(c) = self.idle.channels.iter().find(|c| c.node == i) else {
            return rest;
        };
        c.pose(&rest, t * self.idle.speed)
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

pub fn play(time: Res<Time>, mut rigs: Query<&mut Playing>, mut transforms: Query<&mut Transform>) {
    let dt = time.delta_secs();
    for mut p in &mut rigs {
        let p = &mut *p;
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
        for c in &clip.channels {
            let rest = &p.rig.nodes[c.node].rest;
            if let Ok(mut transform) = transforms.get_mut(p.joints[c.node]) {
                *transform = bevy_transform(&c.pose(rest, t));
            }
        }
    }
}
