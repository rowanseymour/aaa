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
        out.extend((roots.len() as u32).to_le_bytes());
        for r in roots {
            out.extend(r.to_le_bytes());
        }
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
    let tex = nif.property(&shapes[0].properties, |o| match o {
        Object::SourceTexture(t) => Some(t),
        _ => None,
    });
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
    let mut bytes = b"NetImmerse File Format, Version 4.2.2.0\n".to_vec();
    bytes.extend(0x0402_0200u32.to_le_bytes());
    assert!(matches!(
        Nif::parse(&bytes),
        Err(Error::UnsupportedVersion(v)) if v == "4.2.2.0"
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
