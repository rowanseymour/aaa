//! Reader for the `.nif` model files of Axis & Allies (2004).
//!
//! A model is a list of objects (scene nodes, triangle meshes, textures,
//! render properties, animation data) that refer to each other by index; the
//! scene graph hangs off the root objects listed at the end of the file.
//! Nearly all of the game's models are version 10.1.0.0, a few 10.0.1.0 or
//! NetImmerse 4.2.2.0; all are laid out little-endian:
//!
//! ```text
//! header   text line "Gamebryo File Format, Version 10.1.0.0\n",
//!          u32 version (0x0A010000), u32 user version (not in 10.0.1.0),
//!          u32 object count,
//!          u16 type count, type names (u32 length + bytes),
//!          u16 type index per object, u32 group count + u32 groups
//!          [4.2.2.0: no user version and nothing after the count]
//! objects  per object: u32 zero [4.2.2.0: its type name], then its fields;
//!          no sizes are stored, so every type before the last one read
//!          must be understood
//! footer   u32 root count, i32 roots (object indices; -1 = none)
//! ```
//!
//! Strings are a u32 length and Latin-1 bytes, booleans one byte. Object
//! types this reader does not know end the read early: the objects before
//! them are kept and [`Nif::incomplete`] names the type.

use std::fmt;

mod objects;

pub use objects::*;

/// Format versions the reader accepts: 10.1.0.0, 10.0.1.0 and 4.2.2.0.
pub const VERSION: u32 = 0x0A01_0000;
pub const OLD_VERSION: u32 = 0x0A00_0100;
pub const NETIMMERSE_VERSION: u32 = 0x0402_0200;

#[derive(Debug)]
pub enum Error {
    NotNif,
    UnsupportedVersion(String),
    Corrupt(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotNif => write!(f, "not a model file"),
            Error::UnsupportedVersion(v) => write!(f, "unsupported model version {v:?}"),
            Error::Corrupt(s) => write!(f, "corrupt model: {s}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Index of another object in the same file.
pub type Ref = Option<usize>;

pub struct Nif {
    pub objects: Vec<Object>,
    pub roots: Vec<Ref>,
    /// Type of the first object that could not be read, if any; it and
    /// everything after it are missing, and references to them are `None`
    /// through [`Nif::get`].
    pub incomplete: Option<String>,
}

impl Nif {
    pub fn parse(bytes: &[u8]) -> Result<Nif> {
        let line_end = bytes
            .iter()
            .take(128)
            .position(|&b| b == b'\n')
            .ok_or(Error::NotNif)?;
        let line = String::from_utf8_lossy(&bytes[..line_end]);
        if !line.contains("File Format, Version ") {
            return Err(Error::NotNif);
        }
        let mut r = Reader::new(&bytes[line_end + 1..]);
        r.version = r.u32()?;
        if ![VERSION, OLD_VERSION, NETIMMERSE_VERSION].contains(&r.version) {
            let v = line.rsplit(' ').next().unwrap_or_default();
            return Err(Error::UnsupportedVersion(v.to_string()));
        }
        if !r.old() {
            r.u32()?; // user version
        }
        let count = r.u32()? as usize;
        let (types, index) = if r.netimmerse() {
            (Vec::new(), Vec::new())
        } else {
            let type_count = r.u16()? as usize;
            let types = (0..type_count)
                .map(|_| r.string())
                .collect::<Result<Vec<_>>>()?;
            let index = (0..count).map(|_| r.u16()).collect::<Result<Vec<_>>>()?;
            let groups = r.u32()? as usize;
            r.skip(groups * 4)?;
            (types, index)
        };

        // Bounded by the file size, as each object takes at least four bytes.
        let mut objects = Vec::with_capacity(count.min(r.remaining() / 4));
        let mut incomplete = None;
        let mut index = index.into_iter();
        for i in 0..count {
            // 4.2.2.0 has no index; its objects start with their type name.
            let name = match index.next() {
                None => r.string()?,
                Some(t) => {
                    let name = types
                        .get(t as usize & 0x7fff)
                        .ok_or_else(|| Error::Corrupt(format!("type index {t} out of range")))?;
                    if r.u32()? != 0 {
                        return Err(Error::Corrupt(format!("no separator before object {i}")));
                    }
                    name.clone()
                }
            };
            match Object::read(&name, &mut r)? {
                Some(o) => objects.push(o),
                None => {
                    incomplete = Some(name);
                    break;
                }
            }
        }
        let roots = if incomplete.is_none() {
            let n = r.u32()? as usize;
            let roots = (0..n).map(|_| r.reference()).collect::<Result<_>>()?;
            if r.remaining() != 0 {
                return Err(Error::Corrupt(format!("{} trailing bytes", r.remaining())));
            }
            roots
        } else if objects.is_empty() {
            Vec::new()
        } else {
            // The scene root comes first in the game's models.
            vec![Some(0)]
        };
        Ok(Nif {
            objects,
            roots,
            incomplete,
        })
    }

    pub fn get(&self, r: Ref) -> Option<&Object> {
        self.objects.get(r?)
    }

    /// Every visible triangle mesh reachable from the roots, with its transform
    /// relative to the model and the properties in effect for it: its own,
    /// then those inherited from its parents, nearest first.
    pub fn shapes(&self) -> Vec<Placed<'_>> {
        let mut out = Vec::new();
        for &root in &self.roots {
            self.walk(root, Transform::IDENTITY, &mut Vec::new(), &mut out, 0);
        }
        out
    }

    fn walk<'a>(
        &'a self,
        r: Ref,
        parent: Transform,
        props: &mut Vec<Ref>,
        out: &mut Vec<Placed<'a>>,
        depth: usize,
    ) {
        if depth > 64 {
            return;
        }
        let (av, children) = match self.get(r) {
            Some(Object::Node(n)) => (&n.av, Some(&n.children)),
            Some(Object::Shape(s)) => (&s.av, None),
            _ => return,
        };
        if av.hidden() {
            return;
        }
        let transform = parent.then(&av.transform);
        let inherited = props.len();
        props.splice(0..0, av.properties.iter().copied());
        match (self.get(r), children) {
            (Some(Object::Shape(shape)), _) => out.push(Placed {
                shape,
                transform,
                properties: props.clone(),
            }),
            (_, Some(children)) => {
                for &c in children {
                    self.walk(c, transform, props, out, depth + 1);
                }
            }
            _ => {}
        }
        props.drain(0..props.len() - inherited);
    }

    /// The first property of type `T` among `props`.
    pub fn property<'a, T>(
        &'a self,
        props: &[Ref],
        pick: impl Fn(&'a Object) -> Option<&'a T>,
    ) -> Option<&'a T> {
        props.iter().find_map(|&p| self.get(p).and_then(&pick))
    }
}

/// A mesh placed in its model.
pub struct Placed<'a> {
    pub shape: &'a Shape,
    pub transform: Transform,
    pub properties: Vec<Ref>,
}

/// Rotation, then uniform scale, then translation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub translation: [f32; 3],
    /// Rows of the rotation matrix, as stored; it rotates column vectors.
    pub rotation: [[f32; 3]; 3],
    pub scale: f32,
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        translation: [0.0; 3],
        rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        scale: 1.0,
    };

    /// Maps a point from this transform's space to its parent's.
    pub fn apply(&self, p: [f32; 3]) -> [f32; 3] {
        let r = self.rotate(p);
        std::array::from_fn(|i| r[i] * self.scale + self.translation[i])
    }

    pub fn rotate(&self, v: [f32; 3]) -> [f32; 3] {
        self.rotation
            .map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
    }

    /// `child` expressed in this transform's parent space.
    pub fn then(&self, child: &Transform) -> Transform {
        let m = |i: usize, j: usize| {
            (0..3)
                .map(|k| self.rotation[i][k] * child.rotation[k][j])
                .sum()
        };
        Transform {
            translation: self.apply(child.translation),
            rotation: std::array::from_fn(|i| std::array::from_fn(|j| m(i, j))),
            scale: self.scale * child.scale,
        }
    }
}

/// Bounds-checked little-endian reader.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    version: u32,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Reader {
            buf,
            pos: 0,
            version: VERSION,
        }
    }
    /// Whether this is a 10.0.1.0 or 4.2.2.0 file.
    pub(crate) fn old(&self) -> bool {
        self.version < VERSION
    }
    /// Whether this is a 4.2.2.0 file.
    pub(crate) fn netimmerse(&self) -> bool {
        self.version == NETIMMERSE_VERSION
    }
    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        let end =
            end.ok_or_else(|| Error::Corrupt(format!("truncated at byte {} (+{n})", self.pos)))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    pub(crate) fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(drop)
    }
    fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    pub(crate) fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }
    pub(crate) fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub(crate) fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub(crate) fn floats<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }
        Ok(out)
    }
    /// `n` items of `N` floats each.
    pub(crate) fn float_array<const N: usize>(&mut self, n: usize) -> Result<Vec<[f32; N]>> {
        // Check the length up front so a corrupt count cannot allocate much.
        let bytes = self.bytes(n.saturating_mul(N * 4))?;
        Ok(bytes
            .as_chunks::<4>()
            .0
            .as_chunks::<N>()
            .0
            .iter()
            .map(|c| c.map(f32::from_le_bytes))
            .collect())
    }
    pub(crate) fn reference(&mut self) -> Result<Ref> {
        let v = self.u32()? as i32;
        Ok(usize::try_from(v).ok())
    }
    pub(crate) fn references(&mut self) -> Result<Vec<Ref>> {
        let n = self.u32()? as usize;
        self.bytes(n.saturating_mul(4))?;
        self.pos -= n * 4;
        (0..n).map(|_| self.reference()).collect()
    }
    pub(crate) fn string(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        Ok(self.bytes(n)?.iter().map(|&b| b as char).collect())
    }
}

#[cfg(test)]
mod tests;
