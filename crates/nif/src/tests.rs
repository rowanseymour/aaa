use super::*;

/// Writes a model the way the game's files are laid out.
struct Builder {
    types: Vec<&'static str>,
    objects: Vec<(u16, Vec<u8>)>,
}

impl Builder {
    fn new() -> Self {
        Builder {
            types: Vec::new(),
            objects: Vec::new(),
        }
    }

    fn add(&mut self, kind: &'static str, body: Vec<u8>) -> i32 {
        let t = match self.types.iter().position(|&k| k == kind) {
            Some(t) => t,
            None => {
                self.types.push(kind);
                self.types.len() - 1
            }
        };
        self.objects.push((t as u16, body));
        self.objects.len() as i32 - 1
    }

    fn finish(&self, roots: &[i32]) -> Vec<u8> {
        let mut out = b"Gamebryo File Format, Version 10.1.0.0\n".to_vec();
        out.extend(VERSION.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend((self.objects.len() as u32).to_le_bytes());
        out.extend((self.types.len() as u16).to_le_bytes());
        for t in &self.types {
            out.extend(string(t));
        }
        for (t, _) in &self.objects {
            out.extend(t.to_le_bytes());
        }
        out.extend(0u32.to_le_bytes());
        for (_, body) in &self.objects {
            out.extend(0u32.to_le_bytes());
            out.extend(body);
        }
        out.extend(footer(roots));
        out
    }

    /// Writes the model as version 4.2.2.0, which names each object's type
    /// in front of it instead of in a table.
    fn finish_netimmerse(&self, roots: &[i32]) -> Vec<u8> {
        let mut out = b"NetImmerse File Format, Version 4.2.2.0\n".to_vec();
        out.extend(NETIMMERSE_VERSION.to_le_bytes());
        out.extend((self.objects.len() as u32).to_le_bytes());
        for (t, body) in &self.objects {
            out.extend(string(self.types[*t as usize]));
            out.extend(body);
        }
        out.extend(footer(roots));
        out
    }
}

fn footer(roots: &[i32]) -> Vec<u8> {
    let mut v = (roots.len() as u32).to_le_bytes().to_vec();
    v.extend(roots.iter().flat_map(|r| r.to_le_bytes()));
    v
}

fn string(s: &str) -> Vec<u8> {
    let mut v = (s.len() as u32).to_le_bytes().to_vec();
    v.extend(s.as_bytes());
    v
}

fn floats(f: &[f32]) -> Vec<u8> {
    f.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn refs(r: &[i32]) -> Vec<u8> {
    let mut v = (r.len() as u32).to_le_bytes().to_vec();
    v.extend(r.iter().flat_map(|x| x.to_le_bytes()));
    v
}

fn scene(name: &str, flags: u16, translation: [f32; 3], scale: f32, props: &[i32]) -> Vec<u8> {
    let mut v = string(name);
    v.extend(refs(&[]));
    v.extend((-1i32).to_le_bytes());
    v.extend(flags.to_le_bytes());
    v.extend(floats(&translation));
    v.extend(floats(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]));
    v.extend(floats(&[scale]));
    v.extend(refs(props));
    v.extend((-1i32).to_le_bytes());
    v
}

fn node(name: &str, translation: [f32; 3], scale: f32, children: &[i32]) -> Vec<u8> {
    let mut v = scene(name, 0x10, translation, scale, &[]);
    v.extend(refs(children));
    v.extend(refs(&[]));
    v
}

fn shape(name: &str, flags: u16, data: i32, props: &[i32]) -> Vec<u8> {
    let mut v = scene(name, flags, [0.0; 3], 1.0, props);
    v.extend(data.to_le_bytes());
    v.extend((-1i32).to_le_bytes());
    v.push(0);
    v
}

/// Geometry with one triangle, positions and one UV set.
fn geometry(strips: bool) -> Vec<u8> {
    let mut v = 3u16.to_le_bytes().to_vec();
    v.extend([0, 0, 1]);
    v.extend(floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]));
    v.extend(1u16.to_le_bytes());
    v.push(0); // no normals
    v.extend(floats(&[0.0; 4]));
    v.push(0); // no colours
    v.extend(floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
    v.extend(0u16.to_le_bytes());
    v.extend(1u16.to_le_bytes());
    if strips {
        v.extend(1u16.to_le_bytes());
        v.extend(3u16.to_le_bytes());
        v.push(1);
        v.extend([0u16, 1, 2].iter().flat_map(|x| x.to_le_bytes()));
    } else {
        v.extend(3u32.to_le_bytes());
        v.push(1);
        v.extend([0u16, 1, 2].iter().flat_map(|x| x.to_le_bytes()));
        v.extend(0u16.to_le_bytes());
    }
    v
}

fn texture(file: &str) -> Vec<u8> {
    let mut v = string("");
    v.extend(refs(&[]));
    v.extend((-1i32).to_le_bytes());
    v.push(1);
    v.extend(string(file));
    v.extend((-1i32).to_le_bytes());
    v.extend([0; 13]);
    v
}

#[test]
fn reads_a_scene() {
    let mut b = Builder::new();
    let root = b.add("NiNode", node("root", [10.0, 0.0, 0.0], 2.0, &[1, 2, 5]));
    assert_eq!(root, 0);
    b.add("NiTriShape", shape("mesh", 0x10, 3, &[4]));
    b.add("NiTriStrips", shape("strips", 0x10, 6, &[]));
    b.add("NiTriShapeData", geometry(false));
    b.add("NiSourceTexture", texture("bark.dds"));
    b.add("NiTriShape", shape("SelectionBox", 0x11, 3, &[]));
    b.add("NiTriStripsData", geometry(true));
    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert!(nif.incomplete.is_none());

    let shapes = nif.shapes();
    let names: Vec<&str> = shapes.iter().map(|p| p.shape.av.name.as_str()).collect();
    assert_eq!(names, ["mesh", "strips"]);
    assert_eq!(shapes[0].transform.apply([1.0, 0.0, 0.0]), [12.0, 0.0, 0.0]);
    let tex = nif.property::<SourceTexture>(&shapes[0].properties);
    assert_eq!(tex.unwrap().file.as_deref(), Some("bark.dds"));
    for s in &shapes {
        let Some(Object::Geometry(g)) = nif.get(s.shape.data) else {
            panic!("no geometry");
        };
        assert_eq!(g.triangles, [[0, 1, 2]]);
        assert_eq!(g.uv_sets[0][1], [1.0, 0.0]);
    }
}

#[test]
fn stops_at_unknown_types() {
    let mut b = Builder::new();
    b.add("NiNode", node("root", [0.0; 3], 1.0, &[1]));
    b.add("NiSomethingNew", vec![1, 2, 3]);
    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert_eq!(nif.incomplete.as_deref(), Some("NiSomethingNew"));
    assert_eq!(nif.objects.len(), 1);
    assert!(nif.get(Some(1)).is_none());
}

#[test]
fn rejects_other_versions() {
    let mut bytes = b"NetImmerse File Format, Version 4.0.0.2\n".to_vec();
    bytes.extend(0x0400_0002u32.to_le_bytes());
    assert!(matches!(
        Nif::parse(&bytes),
        Err(Error::UnsupportedVersion(v)) if v == "4.0.0.2"
    ));
    assert!(matches!(Nif::parse(b"hello"), Err(Error::NotNif)));
}

#[test]
fn strips_keep_winding() {
    let t: Vec<_> = objects_strip(&[0, 1, 2, 3, 3, 4]);
    assert_eq!(t, [[0, 1, 2], [1, 3, 2]]);
}

fn objects_strip(points: &[u16]) -> Vec<[u16; 3]> {
    crate::objects::strip_triangles(points).collect()
}

#[test]
fn reads_the_older_version() {
    let mut b = Builder::new();
    b.add("NiNode", node("root", [0.0; 3], 1.0, &[]));
    let mut bytes = b.finish(&[0]);
    // 10.0.1.0 has no user version after the version number.
    let at = bytes.iter().position(|&c| c == b'\n').unwrap() + 1;
    bytes[at..at + 4].copy_from_slice(&OLD_VERSION.to_le_bytes());
    bytes.drain(at + 4..at + 8);
    let nif = Nif::parse(&bytes).unwrap();
    assert!(matches!(&nif.objects[0], Object::Node(n) if n.av.name == "root"));
}

#[test]
fn steps_over_skins() {
    let mut b = Builder::new();
    b.add("NiNode", node("root", [0.0; 3], 1.0, &[1, 6]));
    let mut skinned = shape("grass", 0x10, 2, &[]);
    let at = skinned.len() - 5;
    skinned[at..at + 4].copy_from_slice(&3i32.to_le_bytes());
    b.add("NiTriShape", skinned);
    b.add("NiTriShapeData", geometry(false));

    let mut instance = 4i32.to_le_bytes().to_vec();
    instance.extend(0i32.to_le_bytes());
    instance.extend(refs(&[6]));
    b.add("NiSkinInstance", instance);

    let transform = floats(&[
        1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    let mut data = transform.clone();
    data.extend(1u32.to_le_bytes());
    data.extend(5i32.to_le_bytes());
    data.push(1);
    data.extend(&transform);
    data.extend(floats(&[0.0; 4]));
    data.extend(3u16.to_le_bytes());
    for i in 0..3u16 {
        data.extend(i.to_le_bytes());
        data.extend(floats(&[1.0]));
    }
    b.add("NiSkinData", data);

    let mut partition = 1u32.to_le_bytes().to_vec();
    for n in [3u16, 1, 1, 0, 1, 0] {
        partition.extend(n.to_le_bytes()); // counts, then the bone
    }
    partition.push(1);
    partition.extend([0u16, 1, 2].iter().flat_map(|x| x.to_le_bytes()));
    partition.push(1);
    partition.extend(floats(&[1.0; 3]));
    partition.push(1);
    partition.extend([0u16, 1, 2].iter().flat_map(|x| x.to_le_bytes()));
    partition.push(1);
    partition.extend([0u8; 3]);
    b.add("NiSkinPartition", partition);

    b.add("NiNode", node("bone", [0.0; 3], 1.0, &[]));
    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert!(nif.incomplete.is_none());
    let shapes = nif.shapes();
    assert_eq!(shapes.len(), 1);
    assert_eq!(shapes[0].shape.skin, Some(3));
}

/// 4.2.2.0 scene fields: one extra data link, a velocity, and a bounding
/// volume in place of the collision object.
fn netimmerse_scene(name: &str, extra: i32, translation: [f32; 3], props: &[i32]) -> Vec<u8> {
    let mut v = string(name);
    v.extend(extra.to_le_bytes());
    v.extend((-1i32).to_le_bytes());
    v.extend(0x10u16.to_le_bytes());
    v.extend(floats(&translation));
    v.extend(floats(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]));
    v.extend(floats(&[1.0]));
    v.extend(floats(&[0.0; 3]));
    v.extend(refs(props));
    v
}

#[test]
fn reads_netimmerse_models() {
    let mut b = Builder::new();
    let mut root = netimmerse_scene("Scene Root", 1, [0.0; 3], &[2]);
    root.push(0); // no bounding volume
    root.extend(refs(&[3]));
    root.extend(refs(&[]));
    b.add("NiNode", root);

    let mut extra = (-1i32).to_le_bytes().to_vec();
    extra.extend(11u32.to_le_bytes());
    extra.extend(string("zMode10"));
    b.add("NiStringExtraData", extra);

    let mut zbuffer = string("");
    zbuffer.extend([0xff; 8]);
    zbuffer.extend(3u16.to_le_bytes());
    zbuffer.extend(3u32.to_le_bytes());
    b.add("NiZBufferProperty", zbuffer);

    let mut wave = netimmerse_scene("Wave", -1, [0.0, 5.0, 0.0], &[4, 6]);
    wave.push(1);
    wave.extend(4u32.to_le_bytes()); // a union of a sphere and a box
    wave.extend(2u32.to_le_bytes());
    wave.extend(0u32.to_le_bytes());
    wave.extend(floats(&[0.0; 4]));
    wave.extend(1u32.to_le_bytes());
    wave.extend(floats(&[0.0; 15]));
    wave.extend(8i32.to_le_bytes());
    wave.extend((-1i32).to_le_bytes());
    b.add("NiTriStrips", wave);

    let mut texturing = string("");
    texturing.extend([0xff; 8]);
    texturing.extend(0u16.to_le_bytes());
    texturing.extend(2u32.to_le_bytes());
    texturing.extend(1u32.to_le_bytes());
    texturing.push(1);
    texturing.extend(5i32.to_le_bytes());
    texturing.extend([0; 3 * 4 + 4]);
    texturing.extend([0; 4]); // the other slots
    b.add("NiTexturingProperty", texturing);
    let mut source = string("");
    source.extend([0xff; 8]);
    source.push(1);
    source.extend(string("foam.dds"));
    source.extend([0; 13]);
    b.add("NiSourceTexture", source);

    let mut material = string("");
    material.extend((-1i32).to_le_bytes());
    material.extend(7i32.to_le_bytes());
    material.extend(0u16.to_le_bytes());
    material.extend(floats(&[0.5; 14]));
    b.add("NiMaterialProperty", material);
    let mut fade = (-1i32).to_le_bytes().to_vec();
    fade.extend(8u16.to_le_bytes());
    fade.extend(floats(&[1.0, 0.0, 0.0, 2.0]));
    fade.extend(3i32.to_le_bytes());
    fade.extend(9i32.to_le_bytes());
    b.add("NiAlphaController", fade);

    let mut data = 3u16.to_le_bytes().to_vec();
    data.push(1);
    data.extend(floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]));
    data.push(0); // no normals
    data.extend(floats(&[0.0; 4]));
    data.push(0); // no colours
    data.extend(1u16.to_le_bytes());
    data.extend(floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
    data.extend(1u16.to_le_bytes());
    data.extend(1u16.to_le_bytes());
    data.extend(3u16.to_le_bytes());
    data.extend([0u16, 1, 2].iter().flat_map(|x| x.to_le_bytes()));
    b.add("NiTriStripsData", data);

    let mut keys = 2u32.to_le_bytes().to_vec();
    keys.extend(1u32.to_le_bytes());
    keys.extend(floats(&[0.0, 0.0, 2.0, 0.5]));
    b.add("NiFloatData", keys);

    let nif = Nif::parse(&b.finish_netimmerse(&[0])).unwrap();
    assert!(nif.incomplete.is_none());
    let Object::Node(root) = &nif.objects[0] else {
        panic!("no root");
    };
    assert_eq!(root.av.extra_data, [Some(1)]);
    assert!(
        matches!(&nif.objects[1], Object::ExtraData(e) if matches!(&e.value, ExtraValue::String(s) if s == "zMode10"))
    );

    let shapes = nif.shapes();
    assert_eq!(shapes.len(), 1);
    assert_eq!(shapes[0].transform.apply([0.0; 3]), [0.0, 5.0, 0.0]);
    assert_eq!(shapes[0].properties, [Some(4), Some(6), Some(2)]);
    let texturing = nif.property::<Texturing>(&shapes[0].properties);
    let source = texturing.unwrap().base.as_ref().unwrap().source;
    assert!(
        matches!(nif.get(source), Some(Object::SourceTexture(t)) if t.file.as_deref() == Some("foam.dds"))
    );
    let Some(Object::Geometry(g)) = nif.get(shapes[0].shape.data) else {
        panic!("no geometry");
    };
    assert_eq!(g.triangles, [[0, 1, 2]]);
    assert_eq!(g.uv_sets[0][1], [1.0, 0.0]);

    let Some(Object::Material(m)) = nif.get(Some(6)) else {
        panic!("no material");
    };
    let Some(Object::Controller(fade)) = nif.get(m.controller) else {
        panic!("no controller");
    };
    assert_eq!(fade.kind, ControllerKind::Alpha);
    assert_eq!((fade.target, fade.data), (Some(3), Some(9)));
    let Some(Object::Floats(keys)) = nif.get(fade.data) else {
        panic!("no keys");
    };
    assert_eq!(keys.sample(fade.key_time(3.0)), Some(0.25));
}

fn keys<T: Value>(interpolation: Interpolation, keys: &[(f32, T, Option<[T; 2]>)]) -> Keys<T> {
    Keys {
        interpolation,
        keys: keys
            .iter()
            .map(|&(time, value, tangents)| Key {
                time,
                value,
                tangents,
            })
            .collect(),
    }
}

#[test]
fn samples_keys() {
    let linear = keys(
        Interpolation::Linear,
        &[(1.0, [0.0, 2.0], None), (3.0, [4.0, 2.0], None)],
    );
    assert_eq!(linear.sample(0.0), Some([0.0, 2.0]));
    assert_eq!(linear.sample(2.0), Some([2.0, 2.0]));
    assert_eq!(linear.sample(5.0), Some([4.0, 2.0]));
    assert_eq!(Keys::<f32>::default().sample(1.0), None);

    // Flat tangents ease in and out but still pass halfway at the middle.
    let eased = keys(
        Interpolation::Quadratic,
        &[(0.0, 0.0, Some([0.0, 0.0])), (2.0, 1.0, Some([0.0, 0.0]))],
    );
    assert_eq!(eased.sample(1.0), Some(0.5));
    assert!(eased.sample(0.5).unwrap() < 0.25);

    let half_turn = [0.0, 0.0, 0.0, 1.0];
    let turn = keys(
        Interpolation::Linear,
        &[(0.0, [1.0, 0.0, 0.0, 0.0], None), (1.0, half_turn, None)],
    );
    let base = Keyframes {
        rotation: Rotation::Quaternion(turn),
        ..Default::default()
    };
    let t = base.apply(&Transform::IDENTITY, 1.0);
    let p = t.apply([1.0, 0.0, 0.0]);
    assert!((p[0] + 1.0).abs() < 1e-6 && p[1].abs() < 1e-6);
}

#[test]
fn cycles_controller_time() {
    let c = |flags| Controller {
        kind: ControllerKind::Keyframe,
        next: None,
        flags,
        frequency: 1.0,
        phase: 0.0,
        start: 1.0,
        stop: 3.0,
        target: None,
        data: None,
    };
    assert_eq!(c(8).key_time(4.0), 2.0);
    assert_eq!(c(8 | 2).key_time(4.0), 2.0);
    assert_eq!(c(8 | 2).key_time(5.5), 1.5);
    assert_eq!(c(8 | 4).key_time(4.0), 3.0);
}

/// The fields every controller starts with.
fn time_controller(target: i32) -> Vec<u8> {
    let mut v = (-1i32).to_le_bytes().to_vec();
    v.extend(8u16.to_le_bytes());
    v.extend(floats(&[1.0, 0.0, 0.0, 2.0]));
    v.extend(target.to_le_bytes());
    v
}

/// Two particles with positions and, from 10.1.0.0, radii.
fn particles_data(netimmerse: bool) -> Vec<u8> {
    let mut v = 2u16.to_le_bytes().to_vec();
    if !netimmerse {
        v.extend([0, 0]);
    }
    v.push(1);
    v.extend(floats(&[0.0; 6]));
    if !netimmerse {
        v.extend(0u16.to_le_bytes());
    }
    v.push(0); // no normals
    v.extend(floats(&[0.0; 4]));
    v.push(0); // no colours
    if netimmerse {
        v.extend(0u16.to_le_bytes());
        v.extend(floats(&[1.0])); // radius
    } else {
        v.extend(0u16.to_le_bytes());
        v.push(1);
        v.extend(floats(&[1.0, 1.0]));
    }
    v.extend(2u16.to_le_bytes());
    v.push(1);
    v.extend(floats(&[0.5, 0.5]));
    if !netimmerse {
        v.push(0); // no rotations
    }
    v
}

#[test]
fn steps_over_particle_systems() {
    let mut b = Builder::new();
    b.add("NiNode", node("root", [0.0; 3], 1.0, &[1, 3]));
    b.add("NiTriShape", shape("chimney", 0x10, 2, &[]));
    b.add("NiTriShapeData", geometry(false));

    let mut smoke = scene("smoke", 0x10, [0.0; 3], 1.0, &[]);
    smoke[string("smoke").len() + 4..][..4].copy_from_slice(&7i32.to_le_bytes());
    smoke.extend(4i32.to_le_bytes());
    smoke.extend((-1i32).to_le_bytes());
    smoke.push(0); // no shader
    smoke.push(1); // world space
    smoke.extend(refs(&[5, 6]));
    b.add("NiParticleSystem", smoke);

    let mut data = particles_data(false);
    data.extend([0; 2 * 40]);
    data.extend(2u16.to_le_bytes());
    data.extend(0u16.to_le_bytes());
    b.add("NiPSysData", data);

    let modifier = |name| {
        let mut v = string(name);
        v.extend(0u32.to_le_bytes());
        v.extend(3i32.to_le_bytes());
        v.push(1);
        v
    };
    let mut emitter = modifier("Box");
    emitter.extend(floats(&[0.0; 13]));
    emitter.extend(1i32.to_le_bytes());
    emitter.extend(floats(&[1.0, 2.0, 3.0]));
    b.add("NiPSysBoxEmitter", emitter);
    let mut fade = modifier("Fade");
    fade.extend(floats(&[1.0]));
    fade.extend(0u16.to_le_bytes());
    fade.extend(floats(&[1.0]));
    fade.extend(0u16.to_le_bytes());
    b.add("NiPSysGrowFadeModifier", fade);

    let mut ctlr = time_controller(3);
    ctlr.extend(string("Box"));
    ctlr.extend(8i32.to_le_bytes());
    b.add("NiPSysEmitterCtlr", ctlr);
    let mut rate = 1u32.to_le_bytes().to_vec();
    rate.extend(1u32.to_le_bytes());
    rate.extend(floats(&[0.0, 10.0]));
    rate.extend(2u32.to_le_bytes());
    rate.extend(floats(&[0.0]));
    rate.push(1);
    rate.extend(floats(&[2.0]));
    rate.push(0);
    b.add("NiPSysEmitterCtlrData", rate);

    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert!(nif.incomplete.is_none());
    assert!(matches!(&nif.objects[3], Object::Other(k) if k == "NiParticleSystem"));
    let names: Vec<_> = nif
        .shapes()
        .iter()
        .map(|p| p.shape.av.name.clone())
        .collect();
    assert_eq!(names, ["chimney"]);
}

#[test]
fn steps_over_controllers() {
    let mut b = Builder::new();
    let mut root = scene("root", 0x10, [0.0; 3], 1.0, &[]);
    root[string("root").len() + 4..][..4].copy_from_slice(&2i32.to_le_bytes());
    root.extend(refs(&[1]));
    root.extend(refs(&[]));
    b.add("NiNode", root);
    b.add("NiNode", node("target", [0.0; 3], 1.0, &[]));

    let mut look = time_controller(0);
    look.extend(0u16.to_le_bytes());
    look.extend(1i32.to_le_bytes());
    b.add("NiLookAtController", look);

    let mut path = time_controller(0);
    path.extend(0u16.to_le_bytes());
    path.extend(0i32.to_le_bytes());
    path.extend(floats(&[0.0, 0.0]));
    path.extend(0u16.to_le_bytes());
    path.extend(4i32.to_le_bytes());
    path.extend(5i32.to_le_bytes());
    b.add("NiPathController", path);
    let mut positions = 2u32.to_le_bytes().to_vec();
    positions.extend(1u32.to_le_bytes());
    positions.extend(floats(&[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]));
    b.add("NiPosData", positions);
    b.add("NiFloatData", 0u32.to_le_bytes().to_vec());

    let mut vis = time_controller(1);
    vis.extend(7i32.to_le_bytes());
    b.add("NiVisController", vis);
    let mut on_off = 2u32.to_le_bytes().to_vec();
    on_off.extend(floats(&[0.0]));
    on_off.push(1);
    on_off.extend(floats(&[1.0]));
    on_off.push(0);
    b.add("NiVisData", on_off);

    let mut flip = time_controller(9);
    flip.extend(0u32.to_le_bytes());
    flip.extend(floats(&[0.0, 0.5]));
    flip.extend(refs(&[10]));
    b.add("NiFlipController", flip);
    b.add("NiTexturingProperty", {
        let mut t = string("");
        t.extend(refs(&[]));
        t.extend(8i32.to_le_bytes());
        t.extend(2u32.to_le_bytes());
        t.extend(0u32.to_le_bytes());
        t.extend([0; 5]);
        t.extend(0u32.to_le_bytes());
        t
    });
    b.add("NiSourceTexture", texture("frame.dds"));

    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert!(nif.incomplete.is_none());
    assert!(
        matches!(&nif.objects[10], Object::SourceTexture(t) if t.file.as_deref() == Some("frame.dds"))
    );
}

#[test]
fn steps_over_netimmerse_particles() {
    let mut b = Builder::new();
    let mut root = netimmerse_scene("Scene Root", -1, [0.0; 3], &[]);
    root.push(0);
    root.extend(refs(&[1]));
    root.extend(refs(&[]));
    b.add("NiNode", root);

    let mut sparks = netimmerse_scene("sparks", -1, [0.0; 3], &[]);
    sparks[string("sparks").len() + 4..][..4].copy_from_slice(&3i32.to_le_bytes());
    sparks.push(0);
    sparks.extend(2i32.to_le_bytes());
    sparks.extend((-1i32).to_le_bytes());
    b.add("NiRotatingParticles", sparks);
    let mut data = particles_data(true);
    data.push(1);
    data.extend(floats(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]));
    b.add("NiRotatingParticlesData", data);

    let mut ctlr = time_controller(1);
    ctlr.extend(floats(&[0.0; 16]));
    ctlr.push(0);
    ctlr.extend(floats(&[0.0; 3]));
    ctlr.extend([0, 0]);
    ctlr.extend(floats(&[0.0; 3]));
    ctlr.extend((-1i32).to_le_bytes());
    ctlr.extend(0u16.to_le_bytes());
    ctlr.extend(floats(&[1.0]));
    ctlr.extend(0u16.to_le_bytes());
    ctlr.extend(floats(&[0.0, 0.0]));
    ctlr.extend(1u16.to_le_bytes());
    ctlr.extend(1u16.to_le_bytes());
    ctlr.extend([0; 40]);
    ctlr.extend((-1i32).to_le_bytes());
    ctlr.extend(4i32.to_le_bytes());
    ctlr.extend((-1i32).to_le_bytes());
    ctlr.push(0);
    b.add("NiParticleSystemController", ctlr);
    let mut grow = (-1i32).to_le_bytes().to_vec();
    grow.extend(3i32.to_le_bytes());
    grow.extend(floats(&[0.1, 0.5]));
    b.add("NiParticleGrowFade", grow);

    let nif = Nif::parse(&b.finish_netimmerse(&[0])).unwrap();
    assert!(nif.incomplete.is_none());
    assert!(nif.shapes().is_empty());
}
