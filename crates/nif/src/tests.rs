use super::*;

/// Rows of a rotation that leaves things as they are.
const UNROTATED: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// The little-endian bytes of an object's fields, appended in order.
#[derive(Clone, Default)]
struct Bytes(Vec<u8>);

impl Bytes {
    fn new() -> Self {
        Bytes::default()
    }
    fn raw(mut self, bytes: &[u8]) -> Self {
        self.0.extend_from_slice(bytes);
        self
    }
    fn then(self, more: Bytes) -> Self {
        self.raw(&more.0)
    }
    fn zeros(self, n: usize) -> Self {
        self.raw(&vec![0; n])
    }
    fn u8(self, v: u8) -> Self {
        self.raw(&[v])
    }
    fn bool(self, v: bool) -> Self {
        self.u8(v.into())
    }
    fn u16(self, v: u16) -> Self {
        self.raw(&v.to_le_bytes())
    }
    fn u16s(self, v: &[u16]) -> Self {
        v.iter().fold(self, |b, &v| b.u16(v))
    }
    fn u32(self, v: u32) -> Self {
        self.raw(&v.to_le_bytes())
    }
    fn i32(self, v: i32) -> Self {
        self.raw(&v.to_le_bytes())
    }
    fn floats(self, v: &[f32]) -> Self {
        v.iter().fold(self, |b, &v| b.raw(&v.to_le_bytes()))
    }
    fn string(self, s: &str) -> Self {
        self.u32(s.len() as u32).raw(s.as_bytes())
    }
    /// A reference to the object at `index`.
    fn obj(self, index: usize) -> Self {
        self.u32(index as u32)
    }
    /// A reference to no object.
    fn no_obj(self) -> Self {
        self.i32(-1)
    }
    fn maybe_obj(self, index: Option<usize>) -> Self {
        match index {
            Some(i) => self.obj(i),
            None => self.no_obj(),
        }
    }
    /// A key group of `n` linear keys, each a time and its value.
    fn linear_keys(self, n: u32, keys: &[f32]) -> Self {
        self.u32(n).u32(1).floats(keys)
    }
    /// A counted list of references.
    fn objs(self, indices: &[usize]) -> Self {
        let b = self.u32(indices.len() as u32);
        indices.iter().fold(b, |b, &i| b.obj(i))
    }
}

/// Writes a model the way the game's files are laid out.
struct Builder {
    types: Vec<&'static str>,
    objects: Vec<(u16, Bytes)>,
}

impl Builder {
    fn new() -> Self {
        Builder {
            types: Vec::new(),
            objects: Vec::new(),
        }
    }

    /// Adds an object, returning its index.
    fn add(&mut self, kind: &'static str, body: Bytes) -> usize {
        let t = match self.types.iter().position(|&k| k == kind) {
            Some(t) => t,
            None => {
                self.types.push(kind);
                self.types.len() - 1
            }
        };
        self.objects.push((t as u16, body));
        self.objects.len() - 1
    }

    fn finish(&self, roots: &[usize]) -> Vec<u8> {
        self.finish_as(VERSION, roots)
    }

    /// Writes the model as version 10.0.1.0, which has no user version.
    fn finish_old(&self, roots: &[usize]) -> Vec<u8> {
        self.finish_as(OLD_VERSION, roots)
    }

    fn finish_as(&self, version: u32, roots: &[usize]) -> Vec<u8> {
        let mut out = Bytes::new()
            .raw(b"Gamebryo File Format, Version 10.1.0.0\n")
            .u32(version);
        if version != OLD_VERSION {
            out = out.u32(0); // user version
        }
        out = out
            .u32(self.objects.len() as u32)
            .u16(self.types.len() as u16);
        for t in &self.types {
            out = out.string(t);
        }
        for (t, _) in &self.objects {
            out = out.u16(*t);
        }
        out = out.u32(0); // groups
        for (_, body) in &self.objects {
            out = out.u32(0).raw(&body.0);
        }
        out.objs(roots).0
    }

    /// Writes the model as version 4.2.2.0, which names each object's type
    /// in front of it instead of in a table.
    fn finish_netimmerse(&self, roots: &[usize]) -> Vec<u8> {
        let mut out = Bytes::new()
            .raw(b"NetImmerse File Format, Version 4.2.2.0\n")
            .u32(NETIMMERSE_VERSION)
            .u32(self.objects.len() as u32);
        for (t, body) in &self.objects {
            out = out.string(self.types[*t as usize]).raw(&body.0);
        }
        out.objs(roots).0
    }
}

/// The fields of a scene object, unnamed but for `name` and unmoved until
/// set otherwise.
#[derive(Clone)]
struct Scene {
    name: &'static str,
    extra: Vec<usize>,
    controller: Option<usize>,
    flags: u16,
    translation: [f32; 3],
    scale: f32,
    props: Vec<usize>,
}

impl Scene {
    fn new(name: &'static str) -> Self {
        Scene {
            name,
            extra: Vec::new(),
            controller: None,
            flags: 0x10,
            translation: [0.0; 3],
            scale: 1.0,
            props: Vec::new(),
        }
    }
    fn extra(self, extra: &[usize]) -> Self {
        Scene {
            extra: extra.to_vec(),
            ..self
        }
    }
    fn controller(self, controller: usize) -> Self {
        Scene {
            controller: Some(controller),
            ..self
        }
    }
    fn flags(self, flags: u16) -> Self {
        Scene { flags, ..self }
    }
    fn at(self, translation: [f32; 3]) -> Self {
        Scene {
            translation,
            ..self
        }
    }
    fn scale(self, scale: f32) -> Self {
        Scene { scale, ..self }
    }
    fn props(self, props: &[usize]) -> Self {
        Scene {
            props: props.to_vec(),
            ..self
        }
    }

    fn bytes(&self) -> Bytes {
        Bytes::new()
            .string(self.name)
            .objs(&self.extra)
            .maybe_obj(self.controller)
            .u16(self.flags)
            .floats(&self.translation)
            .floats(&UNROTATED)
            .floats(&[self.scale])
            .objs(&self.props)
            .no_obj() // collision
    }

    /// The fields as 4.2.2.0 lays them out, up to the bounding volume: one
    /// extra data link, and a velocity.
    fn netimmerse(&self) -> Bytes {
        Bytes::new()
            .string(self.name)
            .maybe_obj(self.extra.first().copied())
            .maybe_obj(self.controller)
            .u16(self.flags)
            .floats(&self.translation)
            .floats(&UNROTATED)
            .floats(&[self.scale])
            .floats(&[0.0; 3])
            .objs(&self.props)
    }

    fn node(&self, children: &[usize]) -> Bytes {
        self.bytes().objs(children).objs(&[])
    }

    /// A 4.2.2.0 node without a bounding volume.
    fn netimmerse_node(&self, children: &[usize]) -> Bytes {
        self.netimmerse().bool(false).objs(children).objs(&[])
    }

    fn shape(&self, data: usize) -> Bytes {
        self.bytes().obj(data).no_obj().bool(false)
    }

    fn skinned_shape(&self, data: usize, skin: usize) -> Bytes {
        self.bytes().obj(data).obj(skin).bool(false)
    }
}

/// The fields of an unnamed object without extra data.
fn named(controller: Option<usize>) -> Bytes {
    Bytes::new().string("").objs(&[]).maybe_obj(controller)
}

/// [`named`] as 4.2.2.0 lays it out.
fn netimmerse_named(controller: Option<usize>) -> Bytes {
    Bytes::new().string("").no_obj().maybe_obj(controller)
}

/// Geometry with one triangle, positions and one UV set.
fn geometry(strips: bool) -> Bytes {
    let v = Bytes::new()
        .u16(3)
        .u8(0) // keep
        .u8(0) // compress
        .bool(true)
        .floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0])
        .u16(1)
        .bool(false) // no normals
        .floats(&[0.0; 4])
        .bool(false) // no colours
        .floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0])
        .u16(0)
        .u16(1);
    if strips {
        v.u16(1).u16(3).bool(true).u16s(&[0, 1, 2])
    } else {
        v.u32(3).bool(true).u16s(&[0, 1, 2]).u16(0)
    }
}

fn texture(file: &str) -> Bytes {
    named(None).u8(1).string(file).no_obj().zeros(13)
}

#[test]
fn reads_a_scene() {
    let mut b = Builder::new();
    let root = b.add(
        "NiNode",
        Scene::new("root")
            .at([10.0, 0.0, 0.0])
            .scale(2.0)
            .node(&[1, 2, 5]),
    );
    assert_eq!(root, 0);
    b.add("NiTriShape", Scene::new("mesh").props(&[4]).shape(3));
    b.add("NiTriStrips", Scene::new("strips").shape(6));
    b.add("NiTriShapeData", geometry(false));
    b.add("NiSourceTexture", texture("bark.dds"));
    b.add(
        "NiTriShape",
        Scene::new("SelectionBox").flags(0x11).shape(3),
    );
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
    b.add("NiNode", Scene::new("root").node(&[1, 2]));
    b.add("NiTriShape", Scene::new("mesh").shape(3));
    // A shape whose extra data marks it as the mesh's shadow hull.
    b.add("NiTriShape", Scene::new("Sg").extra(&[4]).shape(3));
    b.add("NiTriShapeData", geometry(false));
    b.add("NiIntegerExtraData", Bytes::new().string("Sg").u32(1));
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
    b.add("NiNode", Scene::new("root").node(&[1]));
    b.add("NiSomethingNew", Bytes::new().raw(&[1, 2, 3]));
    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert_eq!(nif.incomplete.as_deref(), Some("NiSomethingNew"));
    assert_eq!(nif.objects.len(), 1);
    assert!(nif.get(Some(1)).is_none());
}

#[test]
fn rejects_other_versions() {
    let bytes = Bytes::new()
        .raw(b"NetImmerse File Format, Version 4.0.0.2\n")
        .u32(0x0400_0002);
    assert!(matches!(
        Nif::parse(&bytes.0),
        Err(e @ Error::UnsupportedVersion { version: 0x0400_0002 }) if e.to_string().ends_with(" \"4.0.0.2\"")
    ));
    assert!(matches!(Nif::parse(b"hello"), Err(Error::NotNif)));

    // The error names the version checked, whatever the text says.
    let bytes = Bytes::new()
        .raw(b"Gamebryo File Format, Version 10.1.0.0\r\n")
        .u32(0x1400_0004);
    assert!(matches!(
        Nif::parse(&bytes.0),
        Err(e @ Error::UnsupportedVersion { version: 0x1400_0004 }) if e.to_string().ends_with(" \"20.0.0.4\"")
    ));
}

#[test]
fn strips_keep_winding() {
    let t: Vec<_> = crate::read::strip_triangles(&[0, 1, 2, 3, 3, 4]).collect();
    assert_eq!(t, [[0, 1, 2], [1, 3, 2]]);
}

#[test]
fn reads_the_older_version() {
    let mut b = Builder::new();
    b.add("NiNode", Scene::new("root").node(&[]));
    let nif = Nif::parse(&b.finish_old(&[0])).unwrap();
    assert!(matches!(&nif.objects[0], Object::Node(n) if n.av.name == "root"));
}

#[test]
fn reads_path_flags_from_older_controllers() {
    let mut b = Builder::new();
    let path = Bytes::new()
        .no_obj()
        .u16(0x708)
        .floats(&[1.0, 0.0, 0.0, 12.0])
        .no_obj()
        .i32(1)
        .floats(&[0.5, 0.02])
        .u16(0)
        .no_obj()
        .no_obj();
    b.add("NiPathController", path);
    let nif = Nif::parse(&b.finish_old(&[])).unwrap();
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
    b.add("NiNode", Scene::new("root").node(&[1, 6]));
    b.add("NiTriShape", Scene::new("grass").skinned_shape(2, 3));
    b.add("NiTriShapeData", geometry(false));
    b.add("NiSkinInstance", Bytes::new().obj(4).obj(0).objs(&[6]));

    let transform = |translation: [f32; 3], scale: f32| {
        Bytes::new()
            .floats(&[1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0])
            .floats(&translation)
            .floats(&[scale])
    };
    let mut data = transform([1.0, 2.0, 3.0], 1.0)
        .u32(1)
        .obj(5)
        .bool(true)
        .then(transform([0.0, 0.0, -0.5], 2.0))
        .floats(&[0.1, 0.2, 0.3, 0.4])
        .u16(3);
    for i in 0..3u16 {
        data = data.u16(i).floats(&[1.0 - f32::from(i) * 0.25]);
    }
    b.add("NiSkinData", data);

    let partition = Bytes::new()
        .u32(1)
        .u16s(&[3, 1, 1, 0, 1, 0]) // counts, then the bone
        .bool(true)
        .u16s(&[0, 1, 2])
        .bool(true)
        .floats(&[1.0; 3])
        .bool(true)
        .u16s(&[0, 1, 2])
        .bool(true)
        .zeros(3);
    b.add("NiSkinPartition", partition);

    b.add("NiNode", Scene::new("bone").node(&[]));
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
    let data = Bytes::new()
        .floats(&UNROTATED)
        .floats(&[0.0, 0.0, 0.0, 1.0])
        .u32(1)
        .no_obj()
        .bool(false)
        .floats(&UNROTATED)
        .floats(&[0.0, 0.0, 0.0, 1.0])
        .floats(&[0.0; 4])
        .u16(7); // vertices, with no weights stored
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
    let data = Bytes::new()
        .floats(&UNROTATED)
        .floats(&[0.0, 0.0, 0.0, 1.0])
        .u32(u32::MAX)
        .no_obj()
        .bool(true);
    let mut b = Builder::new();
    b.add("NiSkinData", data);
    assert!(Nif::parse(&b.finish(&[])).is_err());
}

#[test]
fn reads_netimmerse_models() {
    let mut b = Builder::new();
    let root = Scene::new("Scene Root").extra(&[1]).props(&[2]);
    b.add("NiNode", root.netimmerse_node(&[3]));
    b.add(
        "NiStringExtraData",
        Bytes::new().no_obj().u32(11).string("zMode10"),
    );
    b.add("NiZBufferProperty", netimmerse_named(None).u16(3).u32(3));

    let wave = Scene::new("Wave")
        .at([0.0, 5.0, 0.0])
        .props(&[4, 6])
        .netimmerse()
        .bool(true)
        .u32(4) // a union of a sphere and a box
        .u32(2)
        .u32(0)
        .floats(&[0.0; 4])
        .u32(1)
        .floats(&[0.0; 15])
        .obj(8)
        .no_obj();
    b.add("NiTriStrips", wave);

    let texturing = netimmerse_named(None)
        .u16(0)
        .u32(2)
        .u32(1)
        .bool(true)
        .obj(5)
        .zeros(3 * 4 + 4)
        .zeros(4); // the other slots
    b.add("NiTexturingProperty", texturing);
    let source = netimmerse_named(None).u8(1).string("foam.dds").zeros(13);
    b.add("NiSourceTexture", source);

    let material = netimmerse_named(Some(7)).u16(0).floats(&[0.5; 14]);
    b.add("NiMaterialProperty", material);
    b.add("NiAlphaController", time_controller(3).obj(9));

    let data = Bytes::new()
        .u16(3)
        .bool(true)
        .floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0])
        .bool(false) // no normals
        .floats(&[0.0; 4])
        .bool(false) // no colours
        .u16(1)
        .floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0])
        .u16(1)
        .u16(1)
        .u16(3)
        .u16s(&[0, 1, 2]);
    b.add("NiTriStripsData", data);

    let keys = Bytes::new().linear_keys(2, &[0.0, 0.0, 2.0, 0.5]);
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
fn time_controller(target: usize) -> Bytes {
    Bytes::new()
        .no_obj()
        .u16(8)
        .floats(&[1.0, 0.0, 0.0, 2.0])
        .obj(target)
}

/// Two particles with positions and, from 10.1.0.0, radii.
fn particles_data(netimmerse: bool) -> Bytes {
    let mut v = Bytes::new().u16(2);
    if !netimmerse {
        v = v.u8(0).u8(0); // keep, compress
    }
    v = v.bool(true).floats(&[0.0; 6]);
    if !netimmerse {
        v = v.u16(0);
    }
    v = v
        .bool(false) // no normals
        .floats(&[0.0; 4])
        .bool(false); // no colours
    v = if netimmerse {
        v.u16(0).floats(&[1.0]) // radius
    } else {
        v.u16(0).bool(true).floats(&[1.0, 1.0])
    };
    v = v.u16(2).bool(true).floats(&[0.5, 0.5]);
    if !netimmerse {
        v = v.bool(false); // no rotations
    }
    v
}

#[test]
fn reads_particle_systems() {
    let mut b = Builder::new();
    b.add(
        "NiNode",
        Scene::new("root").at([0.0, 0.0, 2.0]).node(&[1, 3]),
    );
    b.add("NiTriShape", Scene::new("chimney").shape(2));
    b.add("NiTriShapeData", geometry(false));

    let smoke = Scene::new("smoke")
        .controller(7)
        .shape(4)
        .bool(true) // world space
        .objs(&[5, 6]);
    b.add("NiParticleSystem", smoke);

    let data = particles_data(false)
        .zeros(2 * crate::read::PARTICLE_INFO_SIZE)
        .u16(2)
        .u16(0);
    b.add("NiPSysData", data);

    let modifier = |name| Bytes::new().string(name).u32(0).obj(3).bool(true);
    let emitter = modifier("Box")
        .floats(&[
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.1, 0.2, 0.3, 0.4, 11.0, 12.0, 13.0,
        ])
        .obj(1)
        .floats(&[1.0, 2.0, 3.0]);
    b.add("NiPSysBoxEmitter", emitter);
    let fade = modifier("Fade").floats(&[1.0]).u16(0).floats(&[1.0]).u16(0);
    b.add("NiPSysGrowFadeModifier", fade);

    b.add("NiPSysEmitterCtlr", time_controller(3).string("Box").obj(8));
    let rate = Bytes::new()
        .linear_keys(1, &[0.0, 10.0])
        .u32(2)
        .floats(&[0.0])
        .bool(true)
        .floats(&[2.0])
        .bool(false);
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
    let modifier = |name| Bytes::new().string(name).u32(2000).no_obj().bool(false);
    let gravity = modifier("Gravity")
        .obj(3)
        .floats(&[0.0, 0.0, -1.0, 0.5, 9.8])
        .u32(1)
        .floats(&[0.02, 0.9]);
    b.add("NiPSysGravityModifier", gravity);
    b.add("NiPSysColliderManager", modifier("Colliders").obj(2));
    let plane = Bytes::new()
        .floats(&[0.4])
        .bool(false)
        .bool(true)
        .no_obj()
        .obj(1)
        .no_obj()
        .obj(3)
        .floats(&[10.0, 20.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    b.add("NiPSysPlanarCollider", plane);
    b.add("NiNode", Scene::new("ground").node(&[]));

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
    let header = |name| Bytes::new().string(name).u32(0).no_obj().bool(true);
    let base: Vec<f32> = (1..=13).map(|i| i as f32).collect();
    let cylinder = header("Cylinder").floats(&base).obj(9).floats(&[2.0, 5.0]);
    b.add("NiPSysCylinderEmitter", cylinder);
    let sphere = header("Sphere").floats(&base).obj(9).floats(&[3.0]);
    b.add("NiPSysSphereEmitter", sphere);
    let mesh = header("Mesh")
        .floats(&base)
        .objs(&[9])
        .u32(1)
        .u32(3)
        .floats(&[0.0, 1.0, 0.0]);
    b.add("NiPSysMeshEmitter", mesh);
    let spawn = header("Spawn")
        .u16(2)
        .floats(&[0.5])
        .u16(1)
        .u16(4)
        .floats(&[0.1, 0.2, 3.0, 0.3]);
    b.add("NiPSysSpawnModifier", spawn);
    let drag = header("Drag")
        .obj(9)
        .floats(&[0.0, 0.0, 1.0, 0.5, 7.0, 8.0]);
    b.add("NiPSysDragModifier", drag);
    let rotation = header("Rotation")
        .floats(&[0.25])
        .bool(true)
        .floats(&[1.0, 0.0, 0.0]);
    b.add("NiPSysRotationModifier", rotation);
    b.add("NiPSysAgeDeathModifier", header("Age").bool(true).obj(3));
    let ball = Bytes::new()
        .floats(&[0.6])
        .bool(true)
        .bool(false)
        .obj(3)
        .no_obj()
        .no_obj()
        .obj(9)
        .floats(&[4.0]);
    b.add("NiPSysSphericalCollider", ball);
    b.add("NiPSysUpdateCtlr", time_controller(10));
    b.add("NiNode", Scene::new("emitter").node(&[]));
    for kind in [
        "NiPSysModifierActiveCtlr",
        "NiPSysEmitterSpeedCtlr",
        "NiPSysEmitterInitialRadiusCtlr",
    ] {
        b.add(kind, time_controller(10).string("Drag").obj(9));
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
    b.add("NiNode", Scene::new("root").controller(2).node(&[1]));
    b.add("NiNode", Scene::new("target").node(&[]));

    b.add("NiLookAtController", time_controller(0).u16(0).obj(1));
    let path = time_controller(0)
        .u16(0x32)
        .i32(1)
        .floats(&[0.5, 0.02])
        .u16(2)
        .obj(4)
        .obj(5);
    b.add("NiPathController", path);
    let positions = Bytes::new().linear_keys(2, &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    b.add("NiPosData", positions);
    b.add("NiFloatData", Bytes::new().u32(0));

    b.add("NiVisController", time_controller(1).obj(7));
    let on_off = Bytes::new()
        .u32(2)
        .floats(&[0.0])
        .bool(true)
        .floats(&[1.0])
        .bool(false);
    b.add("NiVisData", on_off);

    let flip = time_controller(9).u32(0).floats(&[0.0, 0.5]).objs(&[10]);
    b.add("NiFlipController", flip);
    b.add(
        "NiTexturingProperty",
        named(Some(8)).u32(2).u32(0).zeros(5).u32(0),
    );
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
    assert!(
        matches!(&nif.objects[8], Object::Controller(c) if c.kind == ControllerKind::Unread && c.target == Some(9))
    );
}

#[test]
fn reads_texture_controllers() {
    let mut b = Builder::new();
    b.add(
        "NiTexturingProperty",
        named(Some(1)).u32(2).u32(0).zeros(5).u32(0),
    );
    let scroll = time_controller(0).bool(false).u32(0).u32(1).obj(2);
    b.add("NiTextureTransformController", scroll);
    let keys = Bytes::new().linear_keys(2, &[0.0, 1.0, 2.0, -1.0]);
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
fn old_particle_controller(target: usize, emitter: usize, modifier: usize) -> Bytes {
    time_controller(target)
        .floats(&[2.0, 0.5, 3.0, 0.1, 0.0, 6.0])
        .floats(&[1.0, 0.0, 0.0])
        .floats(&[1.0, 0.5, 0.25, 1.0])
        .floats(&[0.3, 0.0, 0.5])
        .bool(false) // no reset
        .floats(&[60.0, 0.4, 0.1])
        .bool(true)
        .bool(false)
        .floats(&[0.7, 0.7, 0.2])
        .obj(emitter)
        .u16(0)
        .floats(&[1.0])
        .u16(1)
        .floats(&[0.0, 0.0])
        .u16(1)
        .u16(1)
        .floats(&[0.0, 0.0, 1.5, 0.0, 1.0, 0.0, 0.25, 0.4, 2.0])
        .u16(0)
        .u16(1)
        .no_obj()
        .obj(modifier)
        .no_obj()
        .bool(true)
}

/// The links every older-format modifier starts with.
fn old_modifier(next: Option<usize>, controller: usize) -> Bytes {
    Bytes::new().maybe_obj(next).obj(controller)
}

#[test]
fn reads_netimmerse_particles() {
    let mut b = Builder::new();
    b.add("NiNode", Scene::new("Scene Root").netimmerse_node(&[1, 2]));
    b.add(
        "NiNode",
        Scene::new("sparks Emitter")
            .at([0.0, 0.0, 1.0])
            .netimmerse_node(&[]),
    );

    let sparks = Scene::new("sparks")
        .controller(4)
        .netimmerse()
        .bool(false)
        .obj(3)
        .no_obj();
    b.add("NiRotatingParticles", sparks);
    let data = particles_data(true)
        .bool(true)
        .floats(&[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    b.add("NiRotatingParticlesData", data);

    b.add(
        "NiParticleSystemController",
        old_particle_controller(2, 1, 5),
    );
    let spin = old_modifier(Some(6), 4)
        .bool(true)
        .floats(&[0.0, 1.0, 0.0, 18.0]);
    b.add("NiParticleRotation", spin);
    b.add("NiParticleColorModifier", old_modifier(Some(7), 4).obj(8));
    b.add(
        "NiParticleGrowFade",
        old_modifier(None, 4).floats(&[0.1, 0.5]),
    );
    let colors = Bytes::new().linear_keys(2, &[0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
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
    b.add("NiNode", Scene::new("root").node(&[1, 2]));
    b.add("NiNode", Scene::new("mist Emitter").node(&[]));
    b.add(
        "NiParticleMeshes",
        Scene::new("mist").controller(4).shape(3),
    );

    // 10.0.1.0 geometry: no keep or compress bytes, and one radius.
    let data = Bytes::new()
        .u16(1)
        .bool(true)
        .floats(&[0.0, 0.0, 1.0])
        .u16(0)
        .bool(false) // no normals
        .floats(&[0.0; 4])
        .bool(false) // no colours
        .u16(0)
        .floats(&[3.5])
        .u16(1)
        .bool(false) // no sizes
        .bool(true)
        .floats(&[1.0, 0.0, 0.0, 0.0])
        .obj(7);
    b.add("NiParticleMeshesData", data);

    b.add(
        "NiParticleSystemController",
        old_particle_controller(2, 1, 5),
    );
    let gravity = old_modifier(Some(6), 4)
        .floats(&[6.0, 0.9])
        .u32(0)
        .floats(&[0.0, 0.0, 1.0, 1.0, 0.0, 0.0]);
    b.add("NiGravity", gravity);
    b.add("NiParticleMeshModifier", old_modifier(None, 4).objs(&[7]));
    b.add("NiNode", Scene::new("NewRoot").node(&[]));

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
    b.add("NiNode", Scene::new("root").node(&[1]));
    b.add("NiNode", Scene::new("smoke").at([0.0, 0.0, 2.0]).node(&[2]));
    // Hidden, moved and with a property of its own.
    let blizzard = Scene::new("Blizzard").flags(0x11).props(&[4]);
    b.add("NiParticles", blizzard.shape(3));
    b.add("NiTriShapeData", geometry(false));
    b.add("NiSpecularProperty", named(None).u16(0));
    // A mesh outside the scene, its root moved.
    let new_root = Scene::new("NewRoot").at([5.0, 0.0, 0.0]).scale(2.0);
    b.add("NiNode", new_root.node(&[6]));
    b.add(
        "NiTriShape",
        Scene::new("quad").at([1.0, 0.0, 0.0]).shape(3),
    );

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
    let sequence = Bytes::new()
        .string("SoldierJog")
        .string("Bip01")
        .obj(1)
        .u32(2)
        .string("Bip01")
        .obj(2)
        .string("Bip01 Head")
        .no_obj();
    b.add("NiControllerSequence", sequence);

    let text = Bytes::new()
        .string("")
        .u32(2)
        .floats(&[0.0])
        .string("start")
        .floats(&[1.5])
        .string("end");
    b.add("NiTextKeyExtraData", text);

    // No target: it is bound to the model's node of the block's name.
    let ctlr = Bytes::new()
        .no_obj()
        .u16(0x2c)
        .floats(&[1.0, 0.0, 0.0, 1.5])
        .no_obj()
        .obj(3);
    b.add("NiKeyframeController", ctlr);
    let data = Bytes::new()
        .linear_keys(1, &[0.0, 1.0, 0.0, 0.0, 0.0])
        .linear_keys(2, &[0.0, 0.0, 0.0, 0.0, 1.5, 3.0, 0.0, 0.0])
        .u32(0);
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
    let sequence = Bytes::new()
        .string("Walk")
        .string("")
        .no_obj()
        .u32(u32::MAX);
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
    b.add("NiNode", Scene::new("root").node(&[0, 0, 1]));
    b.add("NiTriShape", Scene::new("mesh").shape(3));
    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert_eq!(nif.shapes().len(), 1);

    // Each node lists the next one twice: 2^40 paths without a budget.
    let mut b = Builder::new();
    for i in 0..40 {
        b.add("NiNode", Scene::new("n").node(&[i + 1, i + 1]));
    }
    b.add("NiTriShape", Scene::new("mesh").shape(3));
    let nif = Nif::parse(&b.finish(&[0])).unwrap();
    assert!(nif.shapes().len() <= 41 * VISITS_PER_OBJECT);
}

#[test]
fn reads_a_camera_and_what_follows_it() {
    let camera = Scene::new("PortraitCamera")
        .at([0.0, -12.0, 1.0])
        .bytes()
        .u16(0)
        .floats(&[-0.1, 0.1, 0.2, -0.2, 1.0, 5000.0])
        .bool(false)
        // Its viewport and level of detail, a reference and two unused numbers.
        .floats(&[0.0, 1.0, 1.0, 0.0, 1.0])
        .no_obj()
        .zeros(8);
    let mut b = Builder::new();
    b.add("NiNode", Scene::new("root").node(&[1, 2]));
    b.add("NiCamera", camera);
    b.add("NiNode", Scene::new("after").node(&[]));
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
