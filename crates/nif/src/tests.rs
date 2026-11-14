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
        out.extend(refs(roots));
        out
    }

    /// Writes the model as version 10.0.1.0, which has no user version.
    fn finish_old(&self, roots: &[i32]) -> Vec<u8> {
        let mut bytes = self.finish(roots);
        let at = bytes.iter().position(|&c| c == b'\n').unwrap() + 1;
        bytes[at..at + 4].copy_from_slice(&OLD_VERSION.to_le_bytes());
        bytes.drain(at + 4..at + 8);
        bytes
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
        out.extend(refs(roots));
        out
    }
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
fn leaves_out_shadow_geometry() {
    let mut b = Builder::new();
    b.add("NiNode", node("root", [0.0; 3], 1.0, &[1, 2]));
    b.add("NiTriShape", shape("mesh", 0x10, 3, &[]));
    // A shape whose extra data marks it as the mesh's shadow hull.
    let mut hull = string("Sg");
    hull.extend(refs(&[4]));
    // The rest of a shape, after its empty name and extra data list.
    hull.extend(&shape("", 0x10, 3, &[])[8..]);
    b.add("NiTriShape", hull);
    b.add("NiTriShapeData", geometry(false));
    let mut sg = string("Sg");
    sg.extend(1u32.to_le_bytes());
    b.add("NiIntegerExtraData", sg);
    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert!(nif.incomplete.is_none());

    let names: Vec<&str> = nif
        .shapes()
        .iter()
        .map(|p| p.shape.av.name.as_str())
        .collect();
    assert_eq!(names, ["mesh"]);
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

    // The error names the version checked, whatever the text says.
    let mut bytes = b"Gamebryo File Format, Version 10.1.0.0\r\n".to_vec();
    bytes.extend(0x1400_0004u32.to_le_bytes());
    assert!(matches!(
        Nif::parse(&bytes),
        Err(Error::UnsupportedVersion(v)) if v == "20.0.0.4"
    ));
}

#[test]
fn strips_keep_winding() {
    let t: Vec<_> = objects_strip(&[0, 1, 2, 3, 3, 4]);
    assert_eq!(t, [[0, 1, 2], [1, 3, 2]]);
}

fn objects_strip(points: &[u16]) -> Vec<[u16; 3]> {
    crate::read::strip_triangles(points).collect()
}

#[test]
fn reads_the_older_version() {
    let mut b = Builder::new();
    b.add("NiNode", node("root", [0.0; 3], 1.0, &[]));
    let nif = Nif::parse(&b.finish_old(&[0])).unwrap();
    assert!(matches!(&nif.objects[0], Object::Node(n) if n.av.name == "root"));
}

#[test]
fn reads_path_flags_from_older_controllers() {
    let mut b = Builder::new();
    let mut path = (-1i32).to_le_bytes().to_vec();
    path.extend(0x708u16.to_le_bytes());
    path.extend(floats(&[1.0, 0.0, 0.0, 12.0]));
    path.extend((-1i32).to_le_bytes());
    path.extend(1i32.to_le_bytes());
    path.extend(floats(&[0.5, 0.02]));
    path.extend(0u16.to_le_bytes());
    path.extend((-1i32).to_le_bytes());
    path.extend((-1i32).to_le_bytes());
    b.add("NiPathController", path);
    let mut bytes = b.finish(&[]);
    let at = bytes.iter().position(|&c| c == b'\n').unwrap() + 1;
    bytes[at..at + 4].copy_from_slice(&OLD_VERSION.to_le_bytes());
    bytes.drain(at + 4..at + 8);
    let nif = Nif::parse(&bytes).unwrap();
    assert!(nif.incomplete.is_none());
    let Object::Controller(c) = &nif.objects[0] else {
        panic!("no controller");
    };
    // Active and looping, with the manager-update flag older files imply.
    assert_eq!(c.flags, 0x28);
    assert!(matches!(c.kind, ControllerKind::Path(p) if p.flags == 0x70 && p.follow_axis == 0));
}

#[test]
fn reads_skins() {
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

    let transform = |translation: [f32; 3], scale: f32| {
        let mut v = floats(&[1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0]);
        v.extend(floats(&translation));
        v.extend(floats(&[scale]));
        v
    };
    let mut data = transform([1.0, 2.0, 3.0], 1.0);
    data.extend(1u32.to_le_bytes());
    data.extend(5i32.to_le_bytes());
    data.push(1);
    data.extend(transform([0.0, 0.0, -0.5], 2.0));
    data.extend(floats(&[0.1, 0.2, 0.3, 0.4]));
    data.extend(3u16.to_le_bytes());
    for i in 0..3u16 {
        data.extend(i.to_le_bytes());
        data.extend(floats(&[1.0 - f32::from(i) * 0.25]));
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

    let instance: &SkinInstance = nif.get_as(Some(3)).unwrap();
    assert_eq!(instance.data, Some(4));
    assert_eq!(instance.skeleton_root, Some(0));
    assert_eq!(instance.bones, [Some(6)]);
    let data: &SkinData = nif.get_as(instance.data).unwrap();
    assert_eq!(data.transform.translation, [1.0, 2.0, 3.0]);
    assert_eq!(data.transform.rotation[1], [0.0, 0.0, -1.0]);
    assert_eq!(data.partition, Some(5));
    let [bone] = data.bones.as_slice() else {
        panic!("{:?}", data.bones);
    };
    assert_eq!(bone.transform.translation, [0.0, 0.0, -0.5]);
    assert_eq!(bone.transform.scale, 2.0);
    assert_eq!(bone.bound, [0.1, 0.2, 0.3, 0.4]);
    assert_eq!(bone.weights, [(0, 1.0), (1, 0.75), (2, 0.5)]);
}

#[test]
fn reads_skins_without_weights() {
    let mut data = floats(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    data.extend(floats(&[0.0, 0.0, 0.0, 1.0]));
    data.extend(1u32.to_le_bytes());
    data.extend((-1i32).to_le_bytes());
    data.push(0);
    data.extend(floats(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]));
    data.extend(floats(&[0.0, 0.0, 0.0, 1.0]));
    data.extend(floats(&[0.0; 4]));
    data.extend(7u16.to_le_bytes()); // vertices, with no weights stored
    let mut b = Builder::new();
    b.add("NiSkinData", data);
    let nif = Nif::parse(&b.finish(&[])).unwrap();
    let data: &SkinData = nif.get_as(Some(0)).unwrap();
    assert_eq!(data.partition, None);
    assert_eq!(data.bones.len(), 1);
    assert!(data.bones[0].weights.is_empty());
}

#[test]
fn rejects_corrupt_skins() {
    let mut data = floats(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    data.extend(floats(&[0.0, 0.0, 0.0, 1.0]));
    data.extend(u32::MAX.to_le_bytes());
    data.extend((-1i32).to_le_bytes());
    data.push(1);
    let mut b = Builder::new();
    b.add("NiSkinData", data);
    assert!(Nif::parse(&b.finish(&[])).is_err());
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

/// A path along x, 1 long up to key time 0.9 then 2 more by time 1,
/// constant speed or not, following or not.
fn straight_path(flags: u16) -> Path {
    let settings = PathSettings {
        flags,
        bank_direction: 1,
        max_bank: 0.0,
        smoothing: 0.0,
        follow_axis: 2,
        positions: None,
    };
    let positions = keys(
        Interpolation::Linear,
        &[
            (0.0, [0.0, 0.0, 0.0], None),
            (0.9, [1.0, 0.0, 0.0], None),
            (1.0, [3.0, 0.0, 0.0], None),
        ],
    );
    // The fraction along is the key time itself.
    let percent = keys(
        Interpolation::Linear,
        &[(-10.0, -10.0, None), (10.0, 10.0, None)],
    );
    Path::new(settings, positions, percent)
}

fn x_at(path: &Path, p: f32) -> f32 {
    path.apply(&Transform::IDENTITY, p).translation[0]
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn places_by_distance_along_paths() {
    let even = straight_path(0x10);
    assert!(close(x_at(&even, 0.0), 0.0));
    assert!(close(x_at(&even, 0.25), 0.75));
    assert!(close(x_at(&even, 0.5), 1.5));
    // Without constant speed, by the keys' times instead.
    let timed = straight_path(0);
    assert!(close(x_at(&timed, 0.45), 0.5));
    assert!(close(x_at(&timed, 0.95), 2.0));
    assert!(close(x_at(&timed, 2.0), 3.0));
}

#[test]
fn wraps_constant_speed_paths() {
    let even = straight_path(0x10);
    assert!(close(x_at(&even, 1.25), 0.75));
    assert!(close(x_at(&even, -0.25), 2.25));
    assert!(close(x_at(&even, 1.0), 0.0));
}

#[test]
fn turns_to_follow_paths() {
    // Not following, the rotation is left alone.
    let base = Transform {
        rotation: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        ..Transform::IDENTITY
    };
    let still = straight_path(0x10).apply(&base, 0.5);
    assert_eq!(still.rotation, base.rotation);

    // Along x: z along it, y level across it, and so x straight down.
    let t = straight_path(0x30).apply(&base, 0.5);
    let axis = |i: usize| t.rotation.map(|row| row[i]);
    for (a, b) in [
        (axis(0), [0.0, 0.0, -1.0]),
        (axis(1), [0.0, 1.0, 0.0]),
        (axis(2), [1.0, 0.0, 0.0]),
    ] {
        assert!((0..3).all(|i| close(a[i], b[i])), "{a:?} is not {b:?}");
    }

    // Climbing along y, y still lies level.
    let settings = PathSettings {
        flags: 0x30,
        bank_direction: 1,
        max_bank: 0.0,
        smoothing: 0.0,
        follow_axis: 2,
        positions: None,
    };
    let climb = keys(
        Interpolation::Linear,
        &[(0.0, [0.0, 0.0, 0.0], None), (1.0, [0.0, 1.0, 1.0], None)],
    );
    let percent = keys(Interpolation::Linear, &[(0.0, 0.5, None)]);
    let t = Path::new(settings, climb, percent).apply(&Transform::IDENTITY, 0.0);
    let axis = |i: usize| t.rotation.map(|row| row[i]);
    let h = std::f32::consts::FRAC_1_SQRT_2;
    for (a, b) in [
        (axis(0), [0.0, h, -h]),
        (axis(1), [-1.0, 0.0, 0.0]),
        (axis(2), [0.0, h, h]),
    ] {
        assert!((0..3).all(|i| close(a[i], b[i])), "{a:?} is not {b:?}");
    }
    assert!(close(t.translation[1], 0.5) && close(t.translation[2], 0.5));
}

/// A path followed by `axis` with these flags, through `points` at even
/// key times from 0 to 1, its fraction along the key time itself.
fn followed(flags: u16, axis: i16, points: &[[f32; 3]]) -> Path {
    let settings = PathSettings {
        flags: 0x20 | flags,
        bank_direction: 1,
        max_bank: 0.0,
        smoothing: 0.0,
        follow_axis: axis,
        positions: None,
    };
    let last = (points.len() - 1) as f32;
    let positions = Keys {
        interpolation: Interpolation::Linear,
        keys: points
            .iter()
            .enumerate()
            .map(|(i, &value)| Key {
                time: i as f32 / last,
                value,
                tangents: None,
            })
            .collect(),
    };
    let percent = keys(
        Interpolation::Linear,
        &[(-10.0, -10.0, None), (10.0, 10.0, None)],
    );
    Path::new(settings, positions, percent)
}

/// The node's x, y and z axes where `path` puts it at `p`.
fn axes(path: &Path, p: f32) -> [[f32; 3]; 3] {
    let t = path.apply(&Transform::IDENTITY, p);
    std::array::from_fn(|i| t.rotation.map(|row| row[i]))
}

fn assert_near<const N: usize>(got: [[f32; 3]; N], want: [[f32; 3]; N]) {
    let same = (0..N).all(|a| (0..3).all(|i| close(got[a][i], want[a][i])));
    assert!(same, "{got:?} is not {want:?}");
}

#[test]
fn lays_out_each_follow_axis() {
    let level = [[0.0; 3], [1.0, 0.0, 0.0]];
    let up = [0.0, 0.0, 1.0];
    assert_near(
        axes(&followed(0, 0, &level), 0.5),
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], up],
    );
    assert_near(
        axes(&followed(0, 1, &level), 0.5),
        [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], up],
    );
    assert_near(
        axes(&followed(0, 2, &level), 0.5),
        [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
    );
    // Flipped, x leads backwards and y is reversed, z still up.
    assert_near(
        axes(&followed(0x40, 0, &level), 0.5),
        [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], up],
    );
}

#[test]
fn squashes_nodes_travelling_straight_up() {
    let rising = followed(0, 2, &[[0.0; 3], [0.0, 0.0, 1.0]]);
    assert_near(axes(&rising, 0.5), [[0.0; 3], [0.0; 3], [0.0, 0.0, 1.0]]);
}

#[test]
fn heads_along_each_end_of_a_path() {
    let corner = [[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]];
    let heading = |path: &Path, p| axes(path, p)[2];
    let timed = followed(0, 2, &corner);
    assert_near([heading(&timed, 0.0)], [[1.0, 0.0, 0.0]]);
    // At a key, the way out of it; at the last, the way into it.
    assert_near([heading(&timed, 0.5)], [[0.0, 1.0, 0.0]]);
    assert_near([heading(&timed, 1.0)], [[0.0, 1.0, 0.0]]);
    // At constant speed, p of 1 wraps to the start and its heading.
    let even = followed(0x10, 2, &corner);
    assert_near([heading(&even, 1.0)], [[1.0, 0.0, 0.0]]);
}

#[test]
fn cycles_controller_time() {
    let c = |flags| Controller {
        kind: ControllerKind::Keyframe,
        modifier: String::new(),
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
fn reads_particle_systems() {
    let mut b = Builder::new();
    b.add("NiNode", node("root", [0.0, 0.0, 2.0], 1.0, &[1, 3]));
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
    emitter.extend(floats(&[
        1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.1, 0.2, 0.3, 0.4, 11.0, 12.0, 13.0,
    ]));
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
    let names: Vec<_> = nif
        .shapes()
        .iter()
        .map(|p| p.shape.av.name.clone())
        .collect();
    assert_eq!(names, ["chimney"]);

    let systems = nif.particle_systems();
    let [(system, placed)] = systems.as_slice() else {
        panic!("one particle system");
    };
    assert_eq!(system.shape.av.name, "smoke");
    assert!(system.world_space);
    assert_eq!(system.modifiers, [Some(5), Some(6)]);
    assert_eq!(placed.transform.translation, [0.0, 0.0, 2.0]);
    assert_eq!(placed.path, [0, 3]);
    assert!(matches!(
        nif.get(system.shape.data),
        Some(Object::ParticleData(ParticleData { capacity: 2 }))
    ));

    let Some(Object::Modifier(box_emitter)) = nif.get(Some(5)) else {
        panic!("an emitter");
    };
    assert_eq!(
        (box_emitter.name.as_str(), box_emitter.system),
        ("Box", Some(3))
    );
    let ModifierKind::Emitter(e) = &box_emitter.kind else {
        panic!("an emitter");
    };
    assert_eq!(
        (e.speed, e.planar_angle_variation, e.radius),
        (1.0, 6.0, 11.0)
    );
    assert_eq!(e.color, [0.1, 0.2, 0.3, 0.4]);
    assert_eq!((e.life_span, e.life_span_variation), (12.0, 13.0));
    assert!(matches!(
        e.shape,
        EmitterShape::Box {
            object: Some(1),
            size: [1.0, 2.0, 3.0]
        }
    ));
    assert!(matches!(
        nif.get(Some(6)),
        Some(Object::Modifier(Modifier {
            kind: ModifierKind::GrowFade(GrowFade {
                grow: 1.0,
                fade: 1.0,
                ..
            }),
            ..
        }))
    ));

    let Some(Object::Controller(c)) = nif.get(Some(7)) else {
        panic!("a controller");
    };
    assert_eq!(c.kind, ControllerKind::Emitter);
    assert_eq!(
        (c.modifier.as_str(), c.target, c.data),
        ("Box", Some(3), Some(8))
    );
    let Some(Object::EmitterKeys(keys)) = nif.get(c.data) else {
        panic!("emitter keys");
    };
    assert_eq!(keys.birth_rate.sample(0.0), Some(10.0));
    assert_eq!(keys.active, [(0.0, true), (2.0, false)]);
}

#[test]
fn reads_particle_modifiers_and_colliders() {
    let mut b = Builder::new();
    let modifier = |name| {
        let mut v = string(name);
        v.extend(2000u32.to_le_bytes());
        v.extend((-1i32).to_le_bytes());
        v.push(0);
        v
    };
    let mut gravity = modifier("Gravity");
    gravity.extend(3i32.to_le_bytes());
    gravity.extend(floats(&[0.0, 0.0, -1.0, 0.5, 9.8]));
    gravity.extend(1u32.to_le_bytes());
    gravity.extend(floats(&[0.02, 0.9]));
    b.add("NiPSysGravityModifier", gravity);
    let mut manager = modifier("Colliders");
    manager.extend(2i32.to_le_bytes());
    b.add("NiPSysColliderManager", manager);
    let mut plane = floats(&[0.4]);
    plane.extend([0, 1]);
    for r in [-1i32, 1, -1, 3] {
        plane.extend(r.to_le_bytes());
    }
    plane.extend(floats(&[10.0, 20.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]));
    b.add("NiPSysPlanarCollider", plane);
    b.add("NiNode", node("ground", [0.0; 3], 1.0, &[]));

    let nif = Nif::parse(&b.finish(&[3])).unwrap();
    assert!(nif.incomplete.is_none());
    let Some(Object::Modifier(m)) = nif.get(Some(0)) else {
        panic!("a modifier");
    };
    assert_eq!((m.order, m.active), (2000, false));
    let ModifierKind::Gravity(g) = &m.kind else {
        panic!("gravity");
    };
    assert_eq!(
        (g.object, g.axis, g.decay, g.strength),
        (Some(3), [0.0, 0.0, -1.0], 0.5, 9.8)
    );
    assert_eq!((g.force, g.turbulence, g.turbulence_scale), (1, 0.02, 0.9));
    assert!(matches!(
        nif.get(Some(1)),
        Some(Object::Modifier(Modifier {
            kind: ModifierKind::ColliderManager { collider: Some(2) },
            ..
        }))
    ));
    let Some(Object::Collider(c)) = nif.get(Some(2)) else {
        panic!("a collider");
    };
    assert_eq!(c.bounce, 0.4);
    assert!(!c.spawn_on_collide && c.die_on_collide);
    assert_eq!(
        (c.spawn, c.manager, c.next, c.object),
        (None, Some(1), None, Some(3))
    );
    assert!(matches!(
        c.shape,
        ColliderShape::Plane {
            width: 10.0,
            height: 20.0,
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        }
    ));
}

#[test]
fn reads_the_other_emitters_modifiers_and_controllers() {
    let mut b = Builder::new();
    let header = |name| {
        let mut v = string(name);
        v.extend(0u32.to_le_bytes());
        v.extend((-1i32).to_le_bytes());
        v.push(1);
        v
    };
    let base: Vec<f32> = (1..=13).map(|i| i as f32).collect();
    let mut cylinder = header("Cylinder");
    cylinder.extend(floats(&base));
    cylinder.extend(9i32.to_le_bytes());
    cylinder.extend(floats(&[2.0, 5.0]));
    b.add("NiPSysCylinderEmitter", cylinder);
    let mut sphere = header("Sphere");
    sphere.extend(floats(&base));
    sphere.extend(9i32.to_le_bytes());
    sphere.extend(floats(&[3.0]));
    b.add("NiPSysSphereEmitter", sphere);
    let mut mesh = header("Mesh");
    mesh.extend(floats(&base));
    mesh.extend(refs(&[9]));
    mesh.extend(1u32.to_le_bytes());
    mesh.extend(3u32.to_le_bytes());
    mesh.extend(floats(&[0.0, 1.0, 0.0]));
    b.add("NiPSysMeshEmitter", mesh);
    let mut spawn = header("Spawn");
    spawn.extend(2u16.to_le_bytes());
    spawn.extend(floats(&[0.5]));
    spawn.extend(1u16.to_le_bytes());
    spawn.extend(4u16.to_le_bytes());
    spawn.extend(floats(&[0.1, 0.2, 3.0, 0.3]));
    b.add("NiPSysSpawnModifier", spawn);
    let mut drag = header("Drag");
    drag.extend(9i32.to_le_bytes());
    drag.extend(floats(&[0.0, 0.0, 1.0, 0.5, 7.0, 8.0]));
    b.add("NiPSysDragModifier", drag);
    let mut rotation = header("Rotation");
    rotation.extend(floats(&[0.25]));
    rotation.push(1);
    rotation.extend(floats(&[1.0, 0.0, 0.0]));
    b.add("NiPSysRotationModifier", rotation);
    let mut age = header("Age");
    age.push(1);
    age.extend(3i32.to_le_bytes());
    b.add("NiPSysAgeDeathModifier", age);
    let mut ball = floats(&[0.6]);
    ball.extend([1, 0]);
    for r in [3i32, -1, -1, 9] {
        ball.extend(r.to_le_bytes());
    }
    ball.extend(floats(&[4.0]));
    b.add("NiPSysSphericalCollider", ball);
    b.add("NiPSysUpdateCtlr", time_controller(10));
    b.add("NiNode", node("emitter", [0.0; 3], 1.0, &[]));
    for kind in [
        "NiPSysModifierActiveCtlr",
        "NiPSysEmitterSpeedCtlr",
        "NiPSysEmitterInitialRadiusCtlr",
    ] {
        let mut c = time_controller(10);
        c.extend(string("Drag"));
        c.extend(9i32.to_le_bytes());
        b.add(kind, c);
    }

    let nif = Nif::parse(&b.finish(&[9])).unwrap();
    assert!(nif.incomplete.is_none());
    let kind = |i| match nif.get(Some(i)) {
        Some(Object::Modifier(m)) => &m.kind,
        o => panic!("a modifier, not {o:?}"),
    };
    let shape = |i| match kind(i) {
        ModifierKind::Emitter(e) => {
            assert_eq!(
                (e.speed, e.declination, e.color[3], e.life_span_variation),
                (1.0, 3.0, 10.0, 13.0)
            );
            &e.shape
        }
        k => panic!("an emitter, not {k:?}"),
    };
    assert!(matches!(
        shape(0),
        EmitterShape::Cylinder {
            object: Some(9),
            radius: 2.0,
            height: 5.0
        }
    ));
    assert!(matches!(
        shape(1),
        EmitterShape::Sphere {
            object: Some(9),
            radius: 3.0
        }
    ));
    assert!(matches!(
        shape(2),
        EmitterShape::Mesh { meshes, velocity: 1, emission: 3, axis: [0.0, 1.0, 0.0] }
            if meshes == &[Some(9)]
    ));
    assert!(matches!(
        kind(3),
        ModifierKind::Spawn(Spawn {
            generations: 2,
            percentage: 0.5,
            min: 1,
            max: 4,
            speed_chaos: 0.1,
            direction_chaos: 0.2,
            life_span: 3.0,
            life_span_variation: 0.3,
        })
    ));
    assert!(matches!(
        kind(4),
        ModifierKind::Drag(Drag {
            object: Some(9),
            axis: [0.0, 0.0, 1.0],
            percentage: 0.5,
            range: 7.0,
            falloff: 8.0,
        })
    ));
    assert!(matches!(
        kind(5),
        ModifierKind::Rotation(ParticleRotation {
            speed: 0.25,
            random_axis: true,
            axis: [1.0, 0.0, 0.0]
        })
    ));
    assert!(matches!(
        kind(6),
        ModifierKind::AgeDeath {
            spawn_on_death: true,
            spawn: Some(3)
        }
    ));
    let Some(Object::Collider(c)) = nif.get(Some(7)) else {
        panic!("a collider");
    };
    assert_eq!(
        (c.bounce, c.spawn_on_collide, c.die_on_collide),
        (0.6, true, false)
    );
    assert_eq!((c.spawn, c.object), (Some(3), Some(9)));
    assert!(matches!(c.shape, ColliderShape::Sphere { radius: 4.0 }));

    let controllers: Vec<_> = (8..14)
        .filter_map(|i| nif.get_as::<Controller>(Some(i)))
        .map(|c| (c.kind, c.modifier.as_str(), c.target, c.data))
        .collect();
    assert_eq!(
        controllers,
        [
            (ControllerKind::ParticleUpdate, "", Some(10), None),
            (ControllerKind::ModifierActive, "Drag", Some(10), Some(9)),
            (ControllerKind::EmitterSpeed, "Drag", Some(10), Some(9)),
            (
                ControllerKind::EmitterInitialRadius,
                "Drag",
                Some(10),
                Some(9)
            ),
        ]
    );
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
    path.extend(0x32u16.to_le_bytes());
    path.extend(1i32.to_le_bytes());
    path.extend(floats(&[0.5, 0.02]));
    path.extend(2u16.to_le_bytes());
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
    assert!(matches!(&nif.objects[7], Object::Visibility(v) if v == &[(0.0, true), (1.0, false)]));
    assert!(
        matches!(&nif.objects[10], Object::SourceTexture(t) if t.file.as_deref() == Some("frame.dds"))
    );
    let Object::Controller(path) = &nif.objects[3] else {
        panic!("no path controller");
    };
    assert_eq!(
        path.kind,
        ControllerKind::Path(PathSettings {
            flags: 0x32,
            bank_direction: 1,
            max_bank: 0.5,
            smoothing: 0.02,
            follow_axis: 2,
            positions: Some(4),
        })
    );
    assert_eq!(path.data, Some(5));
    assert!(matches!(&nif.objects[4], Object::Positions(k) if k.keys.len() == 2));
    // Kept so chains through them can be followed.
    assert!(
        matches!(&nif.objects[8], Object::Controller(c) if c.kind == ControllerKind::Unread && c.target == Some(9))
    );
}

#[test]
fn reads_texture_controllers() {
    let mut b = Builder::new();
    b.add("NiTexturingProperty", {
        let mut t = string("");
        t.extend(refs(&[]));
        t.extend(1i32.to_le_bytes());
        t.extend(2u32.to_le_bytes());
        t.extend(0u32.to_le_bytes());
        t.extend([0; 5]);
        t.extend(0u32.to_le_bytes());
        t
    });
    let mut scroll = time_controller(0);
    scroll.push(0);
    scroll.extend(0u32.to_le_bytes());
    scroll.extend(1u32.to_le_bytes());
    scroll.extend(2i32.to_le_bytes());
    b.add("NiTextureTransformController", scroll);
    let mut keys = 2u32.to_le_bytes().to_vec();
    keys.extend(1u32.to_le_bytes());
    keys.extend(floats(&[0.0, 1.0, 2.0, -1.0]));
    b.add("NiFloatData", keys);

    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert!(nif.incomplete.is_none());
    let Some(Object::Texturing(t)) = nif.get(Some(0)) else {
        panic!("no texturing");
    };
    let Some(Object::Controller(c)) = nif.get(t.controller) else {
        panic!("no controller");
    };
    assert_eq!(
        c.kind,
        ControllerKind::Texture {
            slot: 0,
            change: TextureChange::OffsetV
        }
    );
    let Some(Object::Floats(keys)) = nif.get(c.data) else {
        panic!("no keys");
    };
    assert_eq!(keys.sample(c.key_time(1.0)), Some(0.0));
}

/// An older-format particle controller emitting from `emitter`, with
/// modifiers from `modifier` and one saved particle.
fn old_particle_controller(target: i32, emitter: i32, modifier: i32) -> Vec<u8> {
    let mut v = time_controller(target);
    v.extend(floats(&[2.0, 0.5, 3.0, 0.1, 0.0, 6.0]));
    v.extend(floats(&[1.0, 0.0, 0.0]));
    v.extend(floats(&[1.0, 0.5, 0.25, 1.0]));
    v.extend(floats(&[0.3, 0.0, 0.5]));
    v.push(0); // no reset
    v.extend(floats(&[60.0, 0.4, 0.1]));
    v.extend([1, 0]);
    v.extend(floats(&[0.7, 0.7, 0.2]));
    v.extend(emitter.to_le_bytes());
    v.extend(0u16.to_le_bytes());
    v.extend(floats(&[1.0]));
    v.extend(1u16.to_le_bytes());
    v.extend(floats(&[0.0, 0.0]));
    v.extend(1u16.to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(floats(&[0.0, 0.0, 1.5, 0.0, 1.0, 0.0, 0.25, 0.4, 2.0]));
    v.extend(0u16.to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend((-1i32).to_le_bytes());
    v.extend(modifier.to_le_bytes());
    v.extend((-1i32).to_le_bytes());
    v.push(1);
    v
}

/// The links every older-format modifier starts with.
fn old_modifier(next: i32, controller: i32) -> Vec<u8> {
    let mut v = next.to_le_bytes().to_vec();
    v.extend(controller.to_le_bytes());
    v
}

#[test]
fn reads_netimmerse_particles() {
    let mut b = Builder::new();
    let mut root = netimmerse_scene("Scene Root", -1, [0.0; 3], &[]);
    root.push(0);
    root.extend(refs(&[1, 2]));
    root.extend(refs(&[]));
    b.add("NiNode", root);
    let mut emitter = netimmerse_scene("sparks Emitter", -1, [0.0, 0.0, 1.0], &[]);
    emitter.push(0);
    emitter.extend(refs(&[]));
    emitter.extend(refs(&[]));
    b.add("NiNode", emitter);

    let mut sparks = netimmerse_scene("sparks", -1, [0.0; 3], &[]);
    sparks[string("sparks").len() + 4..][..4].copy_from_slice(&4i32.to_le_bytes());
    sparks.push(0);
    sparks.extend(3i32.to_le_bytes());
    sparks.extend((-1i32).to_le_bytes());
    b.add("NiRotatingParticles", sparks);
    let mut data = particles_data(true);
    data.push(1);
    data.extend(floats(&[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0]));
    b.add("NiRotatingParticlesData", data);

    b.add(
        "NiParticleSystemController",
        old_particle_controller(2, 1, 5),
    );
    let mut spin = old_modifier(6, 4);
    spin.push(1);
    spin.extend(floats(&[0.0, 1.0, 0.0, 18.0]));
    b.add("NiParticleRotation", spin);
    let mut color = old_modifier(7, 4);
    color.extend(8i32.to_le_bytes());
    b.add("NiParticleColorModifier", color);
    let mut grow = old_modifier(-1, 4);
    grow.extend(floats(&[0.1, 0.5]));
    b.add("NiParticleGrowFade", grow);
    let mut colors = 2u32.to_le_bytes().to_vec();
    colors.extend(1u32.to_le_bytes());
    colors.extend(floats(&[0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0]));
    b.add("NiColorData", colors);

    let nif = Nif::parse(&b.finish_netimmerse(&[0])).unwrap();
    assert!(nif.incomplete.is_none());
    assert!(nif.shapes().is_empty());

    let Some(Object::OldParticles(sparks)) = nif.get(Some(2)) else {
        panic!("no particles");
    };
    assert_eq!(sparks.shape.av.name, "sparks");
    assert_eq!(sparks.shape.data, Some(3));
    let data: &OldParticleData = nif.get_as(sparks.shape.data).unwrap();
    assert_eq!(data.kind, OldParticleKind::Rotating);
    assert_eq!(data.geometry.positions.len(), 2);
    assert_eq!(data.radii, [1.0]);
    assert_eq!(data.active, 2);
    assert_eq!(data.sizes, [0.5, 0.5]);
    assert_eq!(data.rotations, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]]);

    let Some(Object::OldParticleController(c)) = nif.get(sparks.shape.av.controller) else {
        panic!("no controller");
    };
    // It is found as a controller like any other.
    let timing: &Controller = nif.get_as(sparks.shape.av.controller).unwrap();
    assert_eq!(timing.kind, ControllerKind::OldParticles);
    assert_eq!((timing.target, timing.stop), (Some(2), 2.0));
    assert_eq!(
        [
            c.speed,
            c.speed_variation,
            c.declination,
            c.declination_variation
        ],
        [2.0, 0.5, 3.0, 0.1]
    );
    assert_eq!([c.planar_angle, c.planar_angle_variation], [0.0, 6.0]);
    assert_eq!(c.normal, [1.0, 0.0, 0.0]);
    assert_eq!(c.color, [1.0, 0.5, 0.25, 1.0]);
    assert_eq!([c.size, c.emit_start, c.emit_stop], [0.3, 0.0, 0.5]);
    assert!(!c.reset);
    assert_eq!(
        [c.birth_rate, c.life_span, c.life_span_variation],
        [60.0, 0.4, 0.1]
    );
    assert!(c.use_birth_rate && !c.spawn_on_death);
    assert_eq!((c.emitter, c.emitter_size), (Some(1), [0.7, 0.7, 0.2]));
    assert_eq!(
        (c.spawn_generations, c.spawn_percentage, c.spawn_multiplier),
        (0, 1.0, 1)
    );
    assert_eq!(c.valid, 1);
    assert_eq!(
        c.particles,
        [SavedParticle {
            velocity: [0.0, 0.0, 1.5],
            axis: [0.0, 1.0, 0.0],
            age: 0.25,
            life_span: 0.4,
            last_update: 2.0,
            generation: 0,
            vertex: 1,
        }]
    );
    assert_eq!((c.emitter_modifier, c.collider), (None, None));
    assert!(c.static_bound);

    let chain: Vec<_> = std::iter::successors(c.modifier, |&m| {
        nif.get_as::<OldModifier>(Some(m)).and_then(|m| m.next)
    })
    .map(|m| nif.get_as::<OldModifier>(Some(m)).unwrap())
    .collect();
    assert_eq!(chain.len(), 3);
    assert!(chain.iter().all(|m| m.controller == Some(4)));
    assert!(matches!(
        chain[0].kind,
        OldModifierKind::Rotation {
            random_axis: true,
            axis: [0.0, 1.0, 0.0],
            speed: 18.0
        }
    ));
    let OldModifierKind::Color { data } = chain[1].kind else {
        panic!("no colour modifier");
    };
    let colors: &Keys<[f32; 4]> = nif.get_as(data).unwrap();
    assert_eq!(colors.sample(0.5), Some([0.5, 0.5, 0.5, 0.5]));
    assert!(matches!(
        chain[2].kind,
        OldModifierKind::GrowFade {
            grow: 0.1,
            fade: 0.5
        }
    ));
}

#[test]
fn reads_older_particle_meshes() {
    let mut b = Builder::new();
    b.add("NiNode", node("root", [0.0; 3], 1.0, &[1, 2]));
    b.add("NiNode", node("mist Emitter", [0.0; 3], 1.0, &[]));
    let mut mist = shape("mist", 0x10, 3, &[]);
    mist[string("mist").len() + 4..][..4].copy_from_slice(&4i32.to_le_bytes());
    b.add("NiParticleMeshes", mist);

    // 10.0.1.0 geometry: no keep or compress bytes, and one radius.
    let mut data = 1u16.to_le_bytes().to_vec();
    data.push(1);
    data.extend(floats(&[0.0, 0.0, 1.0]));
    data.extend(0u16.to_le_bytes());
    data.push(0); // no normals
    data.extend(floats(&[0.0; 4]));
    data.push(0); // no colours
    data.extend(0u16.to_le_bytes());
    data.extend(floats(&[3.5]));
    data.extend(1u16.to_le_bytes());
    data.push(0); // no sizes
    data.push(1);
    data.extend(floats(&[1.0, 0.0, 0.0, 0.0]));
    data.extend(7i32.to_le_bytes());
    b.add("NiParticleMeshesData", data);

    b.add(
        "NiParticleSystemController",
        old_particle_controller(2, 1, 5),
    );
    let mut gravity = old_modifier(6, 4);
    gravity.extend(floats(&[6.0, 0.9]));
    gravity.extend(0u32.to_le_bytes());
    gravity.extend(floats(&[0.0, 0.0, 1.0, 1.0, 0.0, 0.0]));
    b.add("NiGravity", gravity);
    let mut meshes = old_modifier(-1, 4);
    meshes.extend(refs(&[7]));
    b.add("NiParticleMeshModifier", meshes);
    b.add("NiNode", node("NewRoot", [0.0; 3], 1.0, &[]));

    let nif = Nif::parse(&b.finish_old(&[0])).unwrap();
    assert!(nif.incomplete.is_none());
    let mist: &OldParticles = nif.get_as(Some(2)).unwrap();
    let data: &OldParticleData = nif.get_as(mist.shape.data).unwrap();
    assert_eq!(data.kind, OldParticleKind::Meshes { node: Some(7) });
    assert_eq!(data.geometry.positions, [[0.0, 0.0, 1.0]]);
    assert_eq!(data.radii, [3.5]);
    assert!(data.sizes.is_empty());
    assert_eq!(data.rotations, [[1.0, 0.0, 0.0, 0.0]]);

    let c: &OldParticleController = nif.get_as(mist.shape.av.controller).unwrap();
    let Some(Object::OldModifier(gravity)) = nif.get(c.modifier) else {
        panic!("no gravity");
    };
    assert!(matches!(
        gravity.kind,
        OldModifierKind::Gravity {
            decay: 6.0,
            strength: 0.9,
            force: 0,
            position: [0.0, 0.0, 1.0],
            direction: [1.0, 0.0, 0.0],
        }
    ));
    let Some(Object::OldModifier(meshes)) = nif.get(gravity.next) else {
        panic!("no meshes");
    };
    assert!(matches!(&meshes.kind, OldModifierKind::Meshes { meshes } if meshes == &[Some(7)]));
    assert_eq!(meshes.next, None);
}

#[test]
fn places_older_particles_at_their_parent() {
    let mut b = Builder::new();
    b.add("NiNode", node("root", [0.0; 3], 1.0, &[1]));
    let mut smoke = scene("smoke", 0x10, [0.0, 0.0, 2.0], 1.0, &[]);
    smoke.extend(refs(&[2]));
    smoke.extend(refs(&[]));
    b.add("NiNode", smoke);
    // Hidden, moved and with a property of its own.
    b.add("NiParticles", shape("Blizzard", 0x11, 3, &[4]));
    b.add("NiTriShapeData", geometry(false));
    b.add("NiSpecularProperty", {
        let mut v = string("");
        v.extend(refs(&[]));
        v.extend((-1i32).to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v
    });
    // A mesh outside the scene, its root moved.
    b.add("NiNode", node("NewRoot", [5.0, 0.0, 0.0], 2.0, &[6]));
    let mut quad = scene("quad", 0x10, [1.0, 0.0, 0.0], 1.0, &[]);
    quad.extend(3i32.to_le_bytes());
    quad.extend((-1i32).to_le_bytes());
    quad.push(0);
    b.add("NiTriShape", quad);

    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    let old = nif.old_particle_systems();
    assert_eq!(old.len(), 1);
    let (particles, placed) = &old[0];
    assert_eq!(particles.shape.av.name, "Blizzard");
    assert_eq!(placed.transform.translation, [0.0, 0.0, 2.0]);
    assert_eq!(placed.properties, [Some(4)]);
    assert_eq!(placed.path, [0, 1, 2]);

    let below = nif.shapes_below(Some(5));
    assert_eq!(below.len(), 1);
    assert_eq!(below[0].transform.translation, [1.0, 0.0, 0.0]);
    assert!(nif.shapes().is_empty());
}

#[test]
fn transforms_undo() {
    let t = Transform {
        translation: [1.0, 2.0, 3.0],
        rotation: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        scale: 2.0,
    };
    let p = t.inverse().apply(t.apply([0.5, -1.0, 4.0]));
    for (a, b) in p.iter().zip([0.5, -1.0, 4.0]) {
        assert!((a - b).abs() < 1e-5);
    }
}

#[test]
fn reads_animation_sequences() {
    let mut b = Builder::new();
    let mut sequence = string("SoldierJog");
    sequence.extend(string("Bip01"));
    sequence.extend(1i32.to_le_bytes());
    sequence.extend(2u32.to_le_bytes());
    sequence.extend(string("Bip01"));
    sequence.extend(2i32.to_le_bytes());
    sequence.extend(string("Bip01 Head"));
    sequence.extend((-1i32).to_le_bytes());
    b.add("NiControllerSequence", sequence);

    let mut text = string("");
    text.extend(2u32.to_le_bytes());
    text.extend(floats(&[0.0]));
    text.extend(string("start"));
    text.extend(floats(&[1.5]));
    text.extend(string("end"));
    b.add("NiTextKeyExtraData", text);

    // No target: it is bound to the model's node of the block's name.
    let mut ctlr = (-1i32).to_le_bytes().to_vec();
    ctlr.extend(0x2cu16.to_le_bytes());
    ctlr.extend(floats(&[1.0, 0.0, 0.0, 1.5]));
    ctlr.extend((-1i32).to_le_bytes());
    ctlr.extend(3i32.to_le_bytes());
    b.add("NiKeyframeController", ctlr);
    let mut data = 1u32.to_le_bytes().to_vec();
    data.extend(1u32.to_le_bytes());
    data.extend(floats(&[0.0, 1.0, 0.0, 0.0, 0.0]));
    data.extend(2u32.to_le_bytes());
    data.extend(1u32.to_le_bytes());
    data.extend(floats(&[0.0, 0.0, 0.0, 0.0, 1.5, 3.0, 0.0, 0.0]));
    data.extend(0u32.to_le_bytes());
    b.add("NiKeyframeData", data);

    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert!(nif.incomplete.is_none());
    let seq = nif.get_as::<Sequence>(nif.roots[0]).unwrap();
    assert_eq!(seq.name, "SoldierJog");
    assert_eq!(seq.accumulation_root, "Bip01");
    let targets: Vec<_> = seq.blocks.iter().map(|b| b.target.as_str()).collect();
    assert_eq!(targets, ["Bip01", "Bip01 Head"]);
    assert_eq!(seq.blocks[1].controller, None);

    let text = nif.get_as::<TextKeys>(seq.text_keys).unwrap();
    let keys: Vec<_> = text
        .keys
        .iter()
        .map(|k| (k.time, k.text.as_str()))
        .collect();
    assert_eq!(keys, [(0.0, "start"), (1.5, "end")]);

    let ctlr = nif.get_as::<Controller>(seq.blocks[0].controller).unwrap();
    assert_eq!(ctlr.key_time(2.0), 1.5);
    let keyframes = nif.get_as::<Keyframes>(ctlr.data).unwrap();
    let t = keyframes.apply(&Transform::IDENTITY, 0.75);
    assert_eq!(t.translation, [1.5, 0.0, 0.0]);
}

#[test]
fn rejects_corrupt_sequences() {
    let mut b = Builder::new();
    let mut sequence = string("Walk");
    sequence.extend(string(""));
    sequence.extend((-1i32).to_le_bytes());
    sequence.extend(u32::MAX.to_le_bytes());
    b.add("NiControllerSequence", sequence);
    assert!(matches!(
        Nif::parse(&b.finish(&[0])),
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn walks_a_cyclic_or_branching_graph_in_bounded_time() {
    // A node that is its own child, twice.
    let mut b = Builder::new();
    b.add("NiNode", node("root", [0.0; 3], 1.0, &[0, 0, 1]));
    b.add("NiTriShape", shape("mesh", 0x10, 3, &[]));
    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert_eq!(nif.shapes().len(), 1);

    // Each node lists the next one twice: 2^40 paths without a budget.
    let mut b = Builder::new();
    for i in 0..40 {
        b.add("NiNode", node("n", [0.0; 3], 1.0, &[i + 1, i + 1]));
    }
    b.add("NiTriShape", shape("mesh", 0x10, 3, &[]));
    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert!(nif.shapes().len() <= 41 * 64);
}

#[test]
fn reads_a_camera_and_what_follows_it() {
    let mut camera = scene("PortraitCamera", 0x10, [0.0, -12.0, 1.0], 1.0, &[]);
    camera.extend(0u16.to_le_bytes());
    camera.extend(floats(&[-0.1, 0.1, 0.2, -0.2, 1.0, 5000.0]));
    camera.push(0);
    // Its viewport and level of detail, a reference and two unused numbers.
    camera.extend(floats(&[0.0, 1.0, 1.0, 0.0, 1.0]));
    camera.extend((-1i32).to_le_bytes());
    camera.extend([0u8; 8]);
    let mut b = Builder::new();
    b.add("NiNode", node("root", [0.0; 3], 1.0, &[1, 2]));
    b.add("NiCamera", camera);
    b.add("NiNode", node("after", [0.0; 3], 1.0, &[]));
    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert!(nif.incomplete.is_none());
    let Object::Camera(camera) = &nif.objects[1] else {
        panic!("no camera");
    };
    assert_eq!(camera.av.name, "PortraitCamera");
    assert_eq!(camera.av.transform.translation, [0.0, -12.0, 1.0]);
    let window = [camera.left, camera.right, camera.top, camera.bottom];
    assert_eq!(window, [-0.1, 0.1, 0.2, -0.2]);
    assert_eq!((camera.near, camera.far), (1.0, 5000.0));
    assert!(!camera.orthographic);
    assert!(matches!(&nif.objects[2], Object::Node(n) if n.av.name == "after"));
}

#[test]
fn setting_a_cycle_keeps_the_other_flags() {
    let mut c = Controller {
        kind: ControllerKind::Keyframe,
        modifier: String::new(),
        next: None,
        flags: 0x2c,
        frequency: 1.0,
        phase: 0.0,
        start: 0.0,
        stop: 1.0,
        target: None,
        data: None,
    };
    assert_eq!((c.cycle(), c.active()), (Cycle::Clamp, true));
    c.set_cycle(Cycle::Loop);
    assert_eq!((c.cycle(), c.flags), (Cycle::Loop, 0x28));
    c.set_cycle(Cycle::Reverse);
    assert_eq!(c.cycle(), Cycle::Reverse);
}

#[test]
fn stored_numbers_read_as_what_they_mean() {
    let pixels = |format| Pixels {
        format,
        mipmaps: Vec::new(),
        data: Vec::new(),
    };
    let formats = [0, 1, 2, 3, 4, 5, 6, 7].map(|f| pixels(f).pixel_format());
    use PixelFormat::*;
    assert_eq!(
        formats,
        [Rgb, Rgba, Palette, Palette, Dxt1, Dxt3, Dxt5, Other(7)]
    );

    let colours = |vertex_mode, lighting| VertexColor {
        vertex_mode,
        lighting,
    };
    let read = |v: VertexColor| (v.stand_for(), v.lit());
    assert_eq!(read(colours(0, 0)), (VertexColors::Ignored, false));
    assert_eq!(read(colours(1, 1)), (VertexColors::Emissive, true));
    assert_eq!(read(colours(2, 1)), (VertexColors::AmbientDiffuse, true));

    let alpha = |source: u16| Alpha {
        flags: 1 | (source << 1),
        threshold: 0,
    };
    let sources = [0, 2, 6, 7].map(|s| alpha(s).source());
    use BlendFactor as F;
    assert_eq!(sources, [F::One, F::Color, F::Alpha, F::Other(7)]);
}
