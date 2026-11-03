//! Actors: the things made of components, such as units, buildings, herds
//! and kingdoms.
//!
//! ```text
//! actor       u32 version (9-14), u32 kingdom, u64 flags,
//!             u32 template_components, u32 components,
//!             each component's state: construction's first, then the others
//!               in bit order, mind's last; none is prefixed with its length
//!             trailer
//! trailer     i32 main_component (-1 for none), v2: u32, v3-11: u32,
//!             v4: u8 flag [string], v5: u8 flag [u32], v6: string,
//!             v8: u32 u32, v10: u32, v11: u32 list x3, u32 n, (u32 x3)[n],
//!             v13: u32
//! updatable   u32 version (0-1), i32 id      most components start with it
//!
//! construction  updatable, u32 version (0-1), u32 n, f32[n],
//!               v1: u32 n, f32[n], string upgrade; v0: u32 u32
//! body          u32 version (0-4), v2: updatable, v1: f32, f32 fraction,
//!               v3: u32, v4: u32
//! mover         updatable, u32 version (0-8), f32[3], f32, f32, v8: f32,
//!               v0-3: f32[3], f32, f32, v1-6: u8, v2: u8, v3: f32[3] f32,
//!               v5: u8 u32, v6: u8 f32[3] u8 f32[3] f32[3]
//! properties    updatable, u32 version (0-1), u32 n, property[n]:
//!     u32 class (0 or 1; -1 for none, with nothing after),
//!     u32 version (0-3), u32, v1: u32, string id, v2: u8 u32,
//!     class 1 from v3: u32 version (0), u32
//! ```
//!
//! The other components' layouts are in their `skip_` functions below.
//! A mind's layout depends on its class, which the template's components
//! decide: an organization's, an aircraft's or an element's.

use super::{Thing, expect, header::check_version};
use crate::{Cursor, Error, Result, Writer};

/// A thing made of components, such as a unit or building.
#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    pub version: u32,
    /// The owning [`Kingdom`](super::Kingdom)'s [`Thing::id`]; 0 for none.
    pub kingdom: u32,
    pub flags: u64,
    /// The components its template gives it, a bit each as in [`COMPONENTS`].
    pub template_components: u32,
    /// The components the record holds state for: the template's, plus any
    /// the game adds, such as a mind.
    pub components: u32,
    pub state: State,
}

/// An actor's components' state and its trailer.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Decoded {
        /// In record order.
        components: Vec<Component>,
        trailer: Trailer,
    },
    /// Kept whole when a component's layout isn't one the game writes in
    /// the maps known.
    Raw(Vec<u8>),
}

/// One component's state.
#[derive(Debug, Clone, PartialEq)]
pub enum Component {
    Construction(Construction),
    Body(Body),
    Mover(Mover),
    Properties(Properties),
    /// Not decoded, but read to its exact end.
    Framed {
        /// Its bit in [`COMPONENTS`].
        bit: u32,
        data: Vec<u8>,
    },
}

/// Actor component names by their bit in [`Actor::components`], as the
/// templates' `[...Component]` sections name them less the suffix.
pub const COMPONENTS: [&str; 31] = [
    "Body",
    "Projectile",
    "Character",
    "Mover",
    "Mind",
    "Performer",
    "Properties",
    "Organization",
    "Element",
    "Economy",
    "Denizen",
    "Supply",
    "Wall",
    "Structure",
    "Settlement",
    "Construction",
    "Recruit",
    "Experience",
    "Builder",
    "MobileDeployment",
    "Defender",
    "Research",
    "Detection",
    "Aircraft",
    "Transport",
    "Lair",
    "Flare",
    "MetaGame",
    "Portal",
    "City",
    "HQ",
];

const BODY: u32 = 0;
const MOVER: u32 = 3;
const MIND: u32 = 4;
const PROPERTIES: u32 = 6;
const ORGANIZATION: u32 = 7;
const ELEMENT: u32 = 8;
const CONSTRUCTION: u32 = 15;
const AIRCRAFT: u32 = 23;

/// Its id among the world's updatable objects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Updatable {
    pub version: u32,
    pub id: i32,
}

/// A building's construction or upgrade progress.
#[derive(Debug, Clone, PartialEq)]
pub struct Construction {
    pub updatable: Updatable,
    pub version: u32,
    pub progress: Vec<f32>,
    /// From version 1 on.
    pub upgrade: Option<Upgrade>,
    /// Before version 1.
    pub unknown: Option<[u32; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Upgrade {
    pub progress: Vec<f32>,
    /// The template being upgraded to; empty for none.
    pub template: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub version: u32,
    /// From version 2 on.
    pub updatable: Option<Updatable>,
    /// From version 1 on.
    pub unknown: Option<f32>,
    /// A fraction the game multiplies by the template's maximum.
    pub fraction: f32,
    /// From version 3 on.
    pub unknown2: Option<u32>,
    /// From version 4 on.
    pub unknown3: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mover {
    pub updatable: Updatable,
    pub version: u32,
    pub unknown_vector: [f32; 3],
    pub unknown: [f32; 2],
    /// From version 8 on: a fraction the game multiplies by a template value.
    pub fraction: Option<f32>,
    /// Before version 4, and ignored.
    pub old: Option<[f32; 3]>,
    pub unknown2: [f32; 2],
    /// Versions 1 to 6.
    pub old_flag: Option<u8>,
    /// From version 2 on.
    pub flag: Option<u8>,
    /// From version 3 on.
    pub unknown3: Option<([f32; 3], f32)>,
    /// From version 5 on; the flag marks the actor in its flags, too.
    pub flag2: Option<(u8, u32)>,
    /// From version 6 on.
    pub v6: Option<MoverV6>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MoverV6 {
    pub flag: u8,
    pub a: [f32; 3],
    pub flag2: u8,
    pub b: [f32; 3],
    pub c: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Properties {
    pub updatable: Updatable,
    pub version: u32,
    /// `None` where the record holds a class of -1.
    pub properties: Vec<Option<Property>>,
}

/// A shared property, such as a nation's technology, the actor has.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    /// 0, or 1 for the kind with an extra value.
    pub class: u32,
    pub version: u32,
    pub unknown: u32,
    /// From version 1 on.
    pub unknown2: Option<u32>,
    /// The shared property data's id.
    pub id: String,
    /// From version 2 on.
    pub unknown3: Option<(u8, u32)>,
    /// Class 1 from version 3 on.
    pub extra: Option<u32>,
}

/// What follows an actor's components.
#[derive(Debug, Clone, PartialEq)]
pub struct Trailer {
    /// The bit of the component that's in charge, or -1.
    pub main_component: i32,
    /// From version 2 on.
    pub unknown: Option<u32>,
    /// Versions 3 to 11.
    pub unknown2: Option<u32>,
    /// From version 4 on.
    pub unknown_string: Option<Option<String>>,
    /// From version 5 on.
    pub unknown3: Option<Option<u32>>,
    /// From version 6 on.
    pub unknown_string2: Option<String>,
    /// From version 8 on.
    pub unknown4: Option<[u32; 2]>,
    /// From version 10 on.
    pub unknown5: Option<u32>,
    /// From version 11 on.
    pub lists: Option<TrailerLists>,
    /// From version 13 on.
    pub unknown6: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrailerLists {
    pub lists: [Vec<u32>; 3],
    pub triples: Vec<[u32; 3]>,
}

impl Actor {
    /// The first version that writes the flags in 64 bits; the layout of
    /// earlier ones is unconfirmed.
    const MIN_VERSION: u32 = 9;
    /// The newest version the game reads.
    const MAX_VERSION: u32 = 14;

    /// Reads an actor whose end is known: its state is kept raw when it
    /// doesn't decode to exactly that end.
    pub(super) fn parse_whole(cur: &mut Cursor, world: u32) -> Result<Self> {
        let mut actor = Actor::parse_header(cur)?;
        let start = cur.pos();
        let decoded = actor.parse_state(cur, world);
        actor.state = match decoded {
            Ok(state) if cur.at_end() => state,
            _ => {
                cur.seek(start);
                State::Raw(cur.rest().to_vec())
            }
        };
        Ok(actor)
    }

    /// Reads an actor in a stream, which only a fully decoded state allows.
    pub(super) fn parse(cur: &mut Cursor, world: u32) -> Result<Self> {
        let mut actor = Actor::parse_header(cur)?;
        actor.state = actor.parse_state(cur, world)?;
        Ok(actor)
    }

    fn parse_header(cur: &mut Cursor) -> Result<Self> {
        let version = cur.u32()?;
        if !(Actor::MIN_VERSION..=Actor::MAX_VERSION).contains(&version) {
            return Err(Error::Corrupt(format!("actor version {version}")));
        }
        Ok(Actor {
            version,
            kingdom: cur.u32()?,
            flags: cur.u64()?,
            template_components: cur.u32()?,
            components: cur.u32()?,
            state: State::Raw(Vec::new()),
        })
    }

    fn parse_state(&self, cur: &mut Cursor, world: u32) -> Result<State> {
        let components = record_order(self.components)
            .map(|bit| self.parse_component(cur, bit, world))
            .collect::<Result<_>>()?;
        Ok(State::Decoded {
            components,
            trailer: Trailer::parse(cur, self.version)?,
        })
    }

    fn parse_component(&self, cur: &mut Cursor, bit: u32, world: u32) -> Result<Component> {
        Ok(match bit {
            CONSTRUCTION => Component::Construction(Construction::parse(cur)?),
            BODY => Component::Body(Body::parse(cur)?),
            MOVER => Component::Mover(Mover::parse(cur)?),
            PROPERTIES => Component::Properties(Properties::parse(cur)?),
            _ => {
                let start = cur.pos();
                let skip = Skip {
                    world,
                    template_components: self.template_components,
                };
                skip.component(cur, bit)?;
                Component::Framed {
                    bit,
                    data: cur.since(start).to_vec(),
                }
            }
        })
    }

    pub(super) fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(self.version);
        w.u32(self.kingdom);
        w.u64(self.flags);
        w.u32(self.template_components);
        w.u32(self.components);
        match &self.state {
            State::Decoded {
                components,
                trailer,
            } => {
                for component in components {
                    component.write(w)?;
                }
                trailer.write(w, self.version)
            }
            State::Raw(raw) => {
                w.bytes(raw);
                Ok(())
            }
        }
    }

    /// The names of the components the record holds.
    pub fn component_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        COMPONENTS
            .iter()
            .enumerate()
            .filter(|&(bit, _)| self.components >> bit & 1 != 0)
            .map(|(_, &name)| name)
    }

    pub fn has(&self, component: &str) -> bool {
        COMPONENTS
            .iter()
            .position(|&c| c == component)
            .is_some_and(|bit| self.components >> bit & 1 != 0)
    }

    /// The decoded components, if the state is.
    pub fn decoded(&self) -> Option<&[Component]> {
        match &self.state {
            State::Decoded { components, .. } => Some(components),
            State::Raw(_) => None,
        }
    }

    /// A component's state, undecoded, if it's one read only to its end.
    pub(super) fn framed(&self, bit: u32) -> Option<&[u8]> {
        self.decoded()?.iter().find_map(|c| match c {
            Component::Framed { bit: b, data } if *b == bit => Some(data.as_slice()),
            _ => None,
        })
    }
}

/// The components' bits in the order their state is recorded.
fn record_order(components: u32) -> impl Iterator<Item = u32> {
    let has = move |bit: u32| components >> bit & 1 != 0;
    let middle = (0..COMPONENTS.len() as u32).filter(|&b| b != CONSTRUCTION && b != MIND);
    [CONSTRUCTION]
        .into_iter()
        .chain(middle)
        .chain([MIND])
        .filter(move |&b| has(b))
}

fn version(cur: &mut Cursor, what: &str, max: u32) -> Result<u32> {
    check_version(what, cur.u32()?, 0..=max)
}

fn u32s(cur: &mut Cursor) -> Result<Vec<u32>> {
    let n = cur.u32()?;
    (0..n).map(|_| cur.u32()).collect()
}

fn write_u32s(w: &mut Writer, vs: &[u32]) {
    w.u32(vs.len() as u32);
    vs.iter().for_each(|&v| w.u32(v));
}

fn f32s(cur: &mut Cursor) -> Result<Vec<f32>> {
    let n = cur.u32()?;
    (0..n).map(|_| cur.f32()).collect()
}

fn write_f32s(w: &mut Writer, vs: &[f32]) {
    w.u32(vs.len() as u32);
    w.f32s(vs);
}

/// `n` items of `size` bytes, checking the count against what's left first.
fn bytes<'a>(cur: &mut Cursor<'a>, n: u32, size: usize) -> Result<&'a [u8]> {
    let len = (n as usize)
        .checked_mul(size)
        .ok_or_else(|| Error::Corrupt(format!("{n} items")))?;
    cur.bytes(len)
}

impl Component {
    fn write(&self, w: &mut Writer) -> Result<()> {
        match self {
            Component::Construction(c) => c.write(w)?,
            Component::Body(b) => b.write(w),
            Component::Mover(m) => m.write(w),
            Component::Properties(p) => p.write(w)?,
            Component::Framed { data, .. } => w.bytes(data),
        }
        Ok(())
    }
}

impl Updatable {
    pub(super) fn parse(cur: &mut Cursor) -> Result<Self> {
        Ok(Updatable {
            version: version(cur, "updatable", 1)?,
            id: cur.i32()?,
        })
    }

    pub(super) fn write(&self, w: &mut Writer) {
        w.u32(self.version);
        w.i32(self.id);
    }
}

impl Construction {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let updatable = Updatable::parse(cur)?;
        let version = version(cur, "construction", 1)?;
        let progress = f32s(cur)?;
        let (upgrade, unknown) = if version >= 1 {
            let progress = f32s(cur)?;
            let template = cur.string()?;
            (Some(Upgrade { progress, template }), None)
        } else {
            (None, Some([cur.u32()?, cur.u32()?]))
        };
        Ok(Construction {
            updatable,
            version,
            progress,
            upgrade,
            unknown,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        self.updatable.write(w);
        w.u32(self.version);
        write_f32s(w, &self.progress);
        if let Some(upgrade) = &self.upgrade {
            write_f32s(w, &upgrade.progress);
            w.string(&upgrade.template)?;
        }
        if let Some(unknown) = self.unknown {
            unknown.iter().for_each(|&v| w.u32(v));
        }
        Ok(())
    }
}

impl Body {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = version(cur, "body", 4)?;
        Ok(Body {
            version,
            updatable: (version >= 2).then(|| Updatable::parse(cur)).transpose()?,
            unknown: (version >= 1).then(|| cur.f32()).transpose()?,
            fraction: cur.f32()?,
            unknown2: (version >= 3).then(|| cur.u32()).transpose()?,
            unknown3: (version >= 4).then(|| cur.u32()).transpose()?,
        })
    }

    fn write(&self, w: &mut Writer) {
        w.u32(self.version);
        if let Some(u) = &self.updatable {
            u.write(w);
        }
        if let Some(v) = self.unknown {
            w.f32(v);
        }
        w.f32(self.fraction);
        for v in [self.unknown2, self.unknown3].into_iter().flatten() {
            w.u32(v);
        }
    }
}

impl Mover {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let updatable = Updatable::parse(cur)?;
        let v = version(cur, "mover", 8)?;
        Ok(Mover {
            updatable,
            version: v,
            unknown_vector: cur.f32s()?,
            unknown: cur.f32s()?,
            fraction: (v >= 8).then(|| cur.f32()).transpose()?,
            old: (v < 4).then(|| cur.f32s()).transpose()?,
            unknown2: cur.f32s()?,
            old_flag: (1..7).contains(&v).then(|| cur.u8()).transpose()?,
            flag: (v >= 2).then(|| cur.u8()).transpose()?,
            unknown3: (v >= 3)
                .then(|| Result::<_>::Ok((cur.f32s()?, cur.f32()?)))
                .transpose()?,
            flag2: (v >= 5)
                .then(|| Result::<_>::Ok((cur.u8()?, cur.u32()?)))
                .transpose()?,
            v6: (v >= 6)
                .then(|| {
                    Result::<_>::Ok(MoverV6 {
                        flag: cur.u8()?,
                        a: cur.f32s()?,
                        flag2: cur.u8()?,
                        b: cur.f32s()?,
                        c: cur.f32s()?,
                    })
                })
                .transpose()?,
        })
    }

    fn write(&self, w: &mut Writer) {
        self.updatable.write(w);
        w.u32(self.version);
        w.f32s(&self.unknown_vector);
        w.f32s(&self.unknown);
        if let Some(v) = self.fraction {
            w.f32(v);
        }
        if let Some(v) = self.old {
            w.f32s(&v);
        }
        w.f32s(&self.unknown2);
        for v in [self.old_flag, self.flag].into_iter().flatten() {
            w.u8(v);
        }
        if let Some((v, f)) = self.unknown3 {
            w.f32s(&v);
            w.f32(f);
        }
        if let Some((flag, v)) = self.flag2 {
            w.u8(flag);
            w.u32(v);
        }
        if let Some(v6) = &self.v6 {
            w.u8(v6.flag);
            w.f32s(&v6.a);
            w.u8(v6.flag2);
            w.f32s(&v6.b);
            w.f32s(&v6.c);
        }
    }
}

impl Properties {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let updatable = Updatable::parse(cur)?;
        let version = version(cur, "properties", 1)?;
        let n = cur.u32()?;
        let properties = (0..n)
            .map(|_| match cur.u32()? {
                u32::MAX => Ok(None),
                class => Property::parse(cur, class).map(Some),
            })
            .collect::<Result<_>>()?;
        Ok(Properties {
            updatable,
            version,
            properties,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        self.updatable.write(w);
        w.u32(self.version);
        w.u32(self.properties.len() as u32);
        for property in &self.properties {
            match property {
                Some(p) => p.write(w)?,
                None => w.u32(u32::MAX),
            }
        }
        Ok(())
    }
}

impl Property {
    /// Reads a property after its class.
    pub(super) fn parse(cur: &mut Cursor, class: u32) -> Result<Self> {
        if class > 1 {
            return Err(Error::Corrupt(format!("property class {class}")));
        }
        let version = version(cur, "property", 3)?;
        Ok(Property {
            class,
            version,
            unknown: cur.u32()?,
            unknown2: (version >= 1).then(|| cur.u32()).transpose()?,
            id: cur.string()?,
            unknown3: (version >= 2)
                .then(|| Result::<_>::Ok((cur.u8()?, cur.u32()?)))
                .transpose()?,
            extra: (class == 1 && version >= 3)
                .then(|| {
                    expect(cur, "property extra version", 0)?;
                    cur.u32()
                })
                .transpose()?,
        })
    }

    /// Writes the property, class first.
    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(self.class);
        self.write_body(w)
    }

    /// Writes the property after its class.
    pub(super) fn write_body(&self, w: &mut Writer) -> Result<()> {
        w.u32(self.version);
        w.u32(self.unknown);
        if let Some(v) = self.unknown2 {
            w.u32(v);
        }
        w.string(&self.id)?;
        if let Some((flag, v)) = self.unknown3 {
            w.u8(flag);
            w.u32(v);
        }
        if let Some(v) = self.extra {
            w.u32(0);
            w.u32(v);
        }
        Ok(())
    }
}

impl Trailer {
    fn parse(cur: &mut Cursor, v: u32) -> Result<Self> {
        let flagged = |cur: &mut Cursor, what: &str| match cur.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(Error::Corrupt(format!("{what} flag is {b}"))),
        };
        Ok(Trailer {
            main_component: cur.i32()?,
            unknown: (v >= 2).then(|| cur.u32()).transpose()?,
            unknown2: (3..12).contains(&v).then(|| cur.u32()).transpose()?,
            unknown_string: (v >= 4)
                .then(|| {
                    flagged(cur, "trailer string")?
                        .then(|| cur.string())
                        .transpose()
                })
                .transpose()?,
            unknown3: (v >= 5)
                .then(|| flagged(cur, "trailer u32")?.then(|| cur.u32()).transpose())
                .transpose()?,
            unknown_string2: (v >= 6).then(|| cur.string()).transpose()?,
            unknown4: (v >= 8)
                .then(|| Result::<_>::Ok([cur.u32()?, cur.u32()?]))
                .transpose()?,
            unknown5: (v >= 10).then(|| cur.u32()).transpose()?,
            lists: (v >= 11)
                .then(|| {
                    let lists = [u32s(cur)?, u32s(cur)?, u32s(cur)?];
                    let n = cur.u32()?;
                    let triples = bytes(cur, n, 12)?
                        .chunks(12)
                        .map(|c| {
                            let [a, b, c] = [0, 4, 8]
                                .map(|i| u32::from_le_bytes(c[i..i + 4].try_into().unwrap()));
                            [a, b, c]
                        })
                        .collect();
                    Result::<_>::Ok(TrailerLists { lists, triples })
                })
                .transpose()?,
            unknown6: (v >= 13).then(|| cur.u32()).transpose()?,
        })
    }

    fn write(&self, w: &mut Writer, v: u32) -> Result<()> {
        let has = |field: bool, from: bool| -> Result<()> {
            match field == from {
                true => Ok(()),
                false => Err(Error::Unwritable(format!(
                    "actor trailer fields don't suit version {v}"
                ))),
            }
        };
        has(self.unknown.is_some(), v >= 2)?;
        has(self.unknown2.is_some(), (3..12).contains(&v))?;
        has(self.unknown_string.is_some(), v >= 4)?;
        has(self.unknown3.is_some(), v >= 5)?;
        has(self.unknown_string2.is_some(), v >= 6)?;
        has(self.unknown4.is_some(), v >= 8)?;
        has(self.unknown5.is_some(), v >= 10)?;
        has(self.lists.is_some(), v >= 11)?;
        has(self.unknown6.is_some(), v >= 13)?;
        w.i32(self.main_component);
        for v in [self.unknown, self.unknown2].into_iter().flatten() {
            w.u32(v);
        }
        if let Some(s) = &self.unknown_string {
            w.u8(s.is_some().into());
            if let Some(s) = s {
                w.string(s)?;
            }
        }
        if let Some(v) = self.unknown3 {
            w.u8(v.is_some().into());
            if let Some(v) = v {
                w.u32(v);
            }
        }
        if let Some(s) = &self.unknown_string2 {
            w.string(s)?;
        }
        if let Some(vs) = self.unknown4 {
            vs.iter().for_each(|&v| w.u32(v));
        }
        if let Some(v) = self.unknown5 {
            w.u32(v);
        }
        if let Some(lists) = &self.lists {
            lists.lists.iter().for_each(|l| write_u32s(w, l));
            w.u32(lists.triples.len() as u32);
            lists.triples.iter().flatten().for_each(|&v| w.u32(v));
        }
        if let Some(v) = self.unknown6 {
            w.u32(v);
        }
        Ok(())
    }
}

/// Reads past the state of components not decoded into types, to find
/// where it ends.
struct Skip {
    /// The game state's version, which some layouts depend on.
    world: u32,
    template_components: u32,
}

impl Skip {
    fn component(&self, cur: &mut Cursor, bit: u32) -> Result<()> {
        match COMPONENTS[bit as usize] {
            "Character" => skip_character(cur),
            "Mind" => self.mind(cur),
            "Performer" => skip_performer(cur),
            "Organization" => self.organization(cur).map(drop),
            "Element" => self.element(cur),
            "Economy" | "Builder" | "Detection" | "Portal" => {
                Updatable::parse(cur)?;
                version(cur, "component", 0).map(drop)
            }
            "Denizen" => skip_denizen(cur),
            "Supply" => skip_supply(cur),
            "Structure" => skip_structure(cur),
            "Recruit" => skip_recruit(cur),
            "Experience" => {
                version(cur, "experience", 0)?;
                cur.u32().map(drop)
            }
            "MobileDeployment" => skip_mobile_deployment(cur),
            "Defender" => skip_defender(cur),
            "Research" => {
                Updatable::parse(cur)?;
                version(cur, "research", 0)?;
                skip_strings(cur)
            }
            "Transport" => skip_transport(cur),
            "Lair" => {
                Updatable::parse(cur)?;
                version(cur, "lair", 0)?;
                u32s(cur)?;
                cur.bytes(8).map(drop)
            }
            "City" => {
                Updatable::parse(cur)?;
                if version(cur, "city", 1)? >= 1 {
                    u32s(cur)?;
                }
                Ok(())
            }
            "HQ" => skip_hq(cur),
            name => Err(Error::Corrupt(format!("{name} component isn't decoded"))),
        }
    }

    /// The minds' class follows the template's components.
    fn mind(&self, cur: &mut Cursor) -> Result<()> {
        let has = |bit: u32| self.template_components >> bit & 1 != 0;
        if has(ORGANIZATION) {
            skip_organization_mind(cur)
        } else if has(AIRCRAFT) {
            Err(Error::Corrupt("aircraft minds aren't decoded".into()))
        } else if has(ELEMENT) {
            skip_element_mind(cur)
        } else {
            Err(Error::Corrupt("mind of no known class".into()))
        }
    }

    /// The damage an element deals.
    fn damage(&self, cur: &mut Cursor) -> Result<()> {
        cur.bytes(8)?;
        cur.string()?;
        cur.bytes(8)?;
        match self.world {
            ..15 => {}
            15..18 => drop(cur.u8()?),
            _ => drop(cur.u32()?),
        }
        if self.world >= 20 {
            cur.u32()?;
        }
        Ok(())
    }

    fn element(&self, cur: &mut Cursor) -> Result<()> {
        let v = version(cur, "element", 3)?;
        if v >= 1 {
            Updatable::parse(cur)?;
        }
        if v <= 1 {
            cur.u8()?;
        }
        if v >= 3 {
            for _ in 0..cur.u32()? {
                self.damage(cur)?;
            }
        }
        Ok(())
    }

    /// Returns the members, each a whole thing record.
    fn organization(&self, cur: &mut Cursor) -> Result<Organization> {
        Updatable::parse(cur)?;
        let v = version(cur, "organization", 10)?;
        version(cur, "organization layout", 0)?;
        let layout = cur.string()?;
        let n = cur.u32()?;
        let slots = (0..n).map(|_| cur.string()).collect::<Result<_>>()?;
        let positions = cur.u32()?;
        let mut members = Vec::new();
        for _ in 0..positions {
            if cur.i32()? >= 0 {
                members.push(Thing::parse_member(cur, self.world)?);
            }
        }
        if (2..=6).contains(&v) {
            bytes(cur, positions, 4)?;
        }
        if v >= 7 {
            (0..positions).try_for_each(|_| cur.string().map(drop))?;
        }
        cur.bytes(12)?;
        if v >= 3 {
            cur.u32()?;
        }
        if v == 0 {
            cur.u32()?;
        }
        bytes(cur, positions, 8)?;
        if v >= 1 {
            bytes(cur, positions, 4)?;
        }
        if v >= 4 {
            cur.bytes(6)?;
            self.damage(cur)?;
        }
        let tail = [(5, 4), (6, 4), (8, 8), (9, 4), (10, 4)];
        for (from, size) in tail {
            if v >= from {
                cur.bytes(size)?;
            }
        }
        Ok(Organization {
            layout,
            slots,
            members,
        })
    }
}

fn skip_strings(cur: &mut Cursor) -> Result<()> {
    (0..cur.u32()?).try_for_each(|_| cur.string().map(drop))
}

/// A layout id, then the layout's own state.
fn skip_layout(cur: &mut Cursor) -> Result<()> {
    cur.string()?;
    version(cur, "layout", 0)?;
    cur.string()?;
    skip_strings(cur)
}

fn skip_ability(cur: &mut Cursor) -> Result<()> {
    let v = version(cur, "ability", 1)?;
    cur.bytes(13)?;
    if v >= 1 {
        u32s(cur)?;
    }
    Ok(())
}

fn skip_character(cur: &mut Cursor) -> Result<()> {
    Updatable::parse(cur)?;
    let v = version(cur, "character", 2)?;
    cur.u32()?;
    if v < 1 {
        cur.u32()?;
    }
    // The animation type and index, then its player's state.
    cur.bytes(8)?;
    version(cur, "animation", 0)?;
    cur.bytes(13)?;
    if v >= 2 {
        cur.u32()?;
    }
    Ok(())
}

fn skip_performer(cur: &mut Cursor) -> Result<()> {
    Updatable::parse(cur)?;
    let v = version(cur, "performer", 9)?;
    if v < 4 {
        return Err(Error::Corrupt(format!("performer version {v}")));
    }
    cur.bytes(24)?;
    if v < 5 {
        u32s(cur)?;
    }
    if v >= 3 {
        cur.bytes(8)?;
    }
    if v >= 1 {
        for _ in 0..cur.u32()? {
            cur.bytes(8)?;
            let more = [(7, 4), (8, 9), (9, 4)];
            for (from, size) in more {
                if v >= from {
                    cur.bytes(size)?;
                }
            }
        }
        cur.u32()?;
    }
    if v >= 6 {
        cur.u32()?;
    }
    if v >= 5 {
        (0..cur.u32()?).try_for_each(|_| skip_ability(cur))?;
    }
    if v >= 7 {
        cur.u8()?;
    }
    Ok(())
}

fn skip_denizen(cur: &mut Cursor) -> Result<()> {
    Updatable::parse(cur)?;
    let v = version(cur, "denizen", 4)?;
    if v >= 1 {
        cur.u8()?;
    }
    if v >= 2 {
        cur.u8()?;
    }
    if v >= 3 {
        let n = cur.u32()?;
        bytes(cur, n, 8)?;
        if v >= 4 {
            cur.u32()?;
        }
        u32s(cur)?;
        u32s(cur)?;
    }
    Ok(())
}

fn skip_supply(cur: &mut Cursor) -> Result<()> {
    let v = version(cur, "supply", 2)?;
    if v >= 1 {
        Updatable::parse(cur)?;
    }
    if v >= 2 {
        cur.bytes(8)?;
    }
    Ok(())
}

fn skip_structure(cur: &mut Cursor) -> Result<()> {
    let v = version(cur, "structure", 13)?;
    if v >= 6 {
        Updatable::parse(cur)?;
    }
    cur.u32()?;
    if v < 1 {
        skip_strings(cur)?;
    }
    if v >= 2 {
        // Queued items hold objects of their own, and no map has any.
        if cur.u32()? != 0 {
            return Err(Error::Corrupt("structure queue isn't decoded".into()));
        }
        cur.bytes(8)?;
        for from in [11, 12, 7] {
            if v >= from {
                cur.u32()?;
            }
        }
    }
    Ok(())
}

fn skip_recruit(cur: &mut Cursor) -> Result<()> {
    Updatable::parse(cur)?;
    let v = version(cur, "recruit", 3)?;
    if v == 0 {
        return Ok(());
    }
    if cur.u8()? != 0 {
        cur.bytes(8)?;
    }
    if v <= 2 {
        for _ in 0..cur.u32()? {
            cur.string()?;
            if cur.u8()? != 0 {
                skip_layout(cur)?;
            }
        }
        cur.u32()?;
        if v >= 2 {
            cur.u8()?;
        }
    }
    Ok(())
}

fn skip_mobile_deployment(cur: &mut Cursor) -> Result<()> {
    Updatable::parse(cur)?;
    let v = version(cur, "mobile deployment", 3)?;
    cur.u32()?;
    if v >= 1 {
        cur.u32()?;
    }
    if v == 2 {
        skip_id_list(cur)?;
    }
    Ok(())
}

fn skip_id_list(cur: &mut Cursor) -> Result<()> {
    version(cur, "id list", 0)?;
    u32s(cur).map(drop)
}

fn skip_defender(cur: &mut Cursor) -> Result<()> {
    Updatable::parse(cur)?;
    let v = version(cur, "defender", 2)?;
    if v >= 1 {
        (0..cur.u32()?).try_for_each(|_| skip_ability(cur))?;
    }
    if v >= 2 {
        let n = cur.u32()?;
        bytes(cur, n, 8)?;
    }
    Ok(())
}

fn skip_transport(cur: &mut Cursor) -> Result<()> {
    let v = version(cur, "transport", 6)?;
    cur.string()?;
    if cur.u8()? != 0 {
        skip_layout(cur)?;
    }
    cur.bytes(18)?;
    if v == 0 {
        u32s(cur)?;
    }
    if v >= 4 {
        cur.u32()?;
    }
    cur.u32()?;
    skip_id_list(cur)?;
    skip_id_list(cur)?;
    if v >= 3 && cur.u8()? != 0 {
        cur.bytes(8)?;
    }
    if v >= 5 {
        cur.string()?;
    }
    if v >= 6 {
        cur.u32()?;
    }
    Ok(())
}

fn skip_hq(cur: &mut Cursor) -> Result<()> {
    Updatable::parse(cur)?;
    let v = version(cur, "hq", 3)?;
    // The badge.
    cur.i32()?;
    match v {
        1 => drop(u32s(cur)?),
        // Version 0 records end here in every map, though the game's
        // loader reads on as for version 2.
        0 => {}
        _ => {
            for _ in 0..cur.u32()? {
                if cur.u8()? != 0 {
                    cur.bytes(if v < 3 { 5 } else { 12 })?;
                }
            }
        }
    }
    Ok(())
}

/// What every mind's state starts with.
fn skip_mind_base(cur: &mut Cursor) -> Result<()> {
    Updatable::parse(cur)?;
    let v = version(cur, "mind", 3)?;
    cur.bytes(8)?;
    (0..cur.u32()?).try_for_each(|_| skip_command(cur))?;
    if v >= 1 {
        cur.u32()?;
        // The names of its mind states.
        skip_strings(cur)?;
    }
    if v >= 3 && cur.u8()? != 0 {
        skip_command(cur)?;
    }
    Ok(())
}

/// An order queued for an actor.
fn skip_command(cur: &mut Cursor) -> Result<()> {
    let v = version(cur, "command", 3)?;
    cur.string()?;
    let flags = cur.u32()?;
    cur.u32()?;
    cur.string()?;
    cur.bytes(21)?;
    if v >= 3 {
        cur.u8()?;
    }
    if v >= 1 {
        cur.string()?;
    }
    if flags & 1 << 5 != 0 {
        skip_layout(cur)?;
    }
    if v >= 2 {
        cur.string()?;
    }
    Ok(())
}

fn skip_path(cur: &mut Cursor) -> Result<()> {
    version(cur, "path", 0)?;
    let n = cur.u32()?;
    bytes(cur, n, 36)?;
    cur.u32().map(drop)
}

fn skip_element_mind(cur: &mut Cursor) -> Result<()> {
    let v = version(cur, "element mind", 14)?;
    if v == 0 {
        return Ok(());
    }
    skip_mind_base(cur)?;
    if v < 2 {
        return Ok(());
    }
    cur.bytes(8)?;
    if v >= 9 {
        cur.u32()?;
    }
    cur.bytes(52)?;
    if v >= 12 {
        cur.u32()?;
    }
    skip_path(cur)?;
    if v >= 10 {
        cur.bytes(24)?;
    }
    if (5..7).contains(&v) {
        cur.u8()?;
    }
    if v >= 7 {
        cur.u32()?;
    }
    cur.bytes(8)?;
    if v < 4 {
        cur.bytes(29)?;
    }
    cur.bytes(26)?;
    if v >= 8 {
        cur.u8()?;
    }
    cur.bytes(12)?;
    if v >= 3 {
        cur.u32()?;
    }
    if v >= 4 {
        cur.u32()?;
        if v < 6 {
            cur.u8()?;
        }
        cur.bytes(12)?;
        cur.string()?;
    }
    if v >= 9 {
        cur.bytes(12)?;
        if v >= 13 {
            cur.bytes(8)?;
        }
    }
    if v >= 11 {
        cur.bytes(20)?;
    }
    if v >= 14 {
        cur.bytes(8)?;
    }
    Ok(())
}

fn skip_organization_mind(cur: &mut Cursor) -> Result<()> {
    let v = version(cur, "organization mind", 21)?;
    if v == 0 {
        return Ok(());
    }
    skip_mind_base(cur)?;
    cur.bytes(8)?;
    if v < 2 {
        return Ok(());
    }
    u32s(cur)?;
    cur.u32()?;
    if v >= 9 {
        cur.bytes(5)?;
    }
    if v >= 12 {
        cur.u8()?;
    }
    cur.bytes(37)?;
    if v >= 15 {
        cur.bytes(8)?;
    }
    skip_path(cur)?;
    if v >= 3 {
        cur.u32()?;
    }
    // Every map has a u32 here, 0, that the game's loader doesn't
    // appear to read; its place between the path and the string below
    // is a guess.
    cur.bytes(16)?;
    if v < 8 {
        cur.u8()?;
    }
    cur.u32()?;
    if v >= 6 {
        cur.string()?;
        cur.bytes(8)?;
    }
    if v >= 4 {
        cur.bytes(8)?;
        if (5..13).contains(&v) {
            cur.u32()?;
        }
    }
    cur.bytes(13)?;
    cur.string()?;
    cur.bytes(8)?;
    if v >= 11 {
        cur.u32()?;
    }
    if v >= 20 {
        cur.bytes(8)?;
    }
    cur.bytes(9)?;
    if v >= 10 {
        cur.u8()?;
    }
    cur.bytes(45)?;
    let n = cur.u32()?;
    bytes(cur, n, 8)?;
    if v >= 18 {
        cur.u32()?;
    }
    if v >= 19 {
        cur.u8()?;
    }
    if v >= 5 {
        cur.u8()?;
    }
    cur.bytes(52)?;
    if v >= 17 {
        cur.bytes(12)?;
    }
    cur.bytes(9)?;
    if v >= 7 {
        cur.u8()?;
        u32s(cur)?;
    }
    cur.u32()?;
    if v >= 14 {
        cur.bytes(12)?;
        if v >= 21 {
            cur.bytes(8)?;
        }
        if v >= 16 {
            cur.u32()?;
        }
        cur.u32()?;
    }
    Ok(())
}

/// The parts of a herd or regiment, as its record stores them.
#[derive(Debug, Clone)]
pub struct Organization {
    /// The `OrganizationLayout` the members are arranged by.
    pub layout: String,
    /// Each layout slot's element template; empty for an empty slot.
    pub slots: Vec<String>,
    /// The members where they stand, in layout position order.
    pub members: Vec<Thing>,
}

impl Actor {
    /// The organization this actor is, if its record holds one.
    pub fn organization(&self, world: u32) -> Option<Organization> {
        let data = self.framed(ORGANIZATION)?;
        let skip = Skip {
            world,
            template_components: self.template_components,
        };
        skip.organization(&mut Cursor::new(data)).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Detail;

    const WORLD: u32 = 28;

    fn trailer() -> Trailer {
        Trailer {
            main_component: 3,
            unknown: Some(1),
            unknown2: None,
            unknown_string: Some(Some("order".into())),
            unknown3: Some(None),
            unknown_string2: Some(String::new()),
            unknown4: Some([2, 3]),
            unknown5: Some(4),
            lists: Some(TrailerLists {
                lists: [vec![5], vec![], vec![6, 7]],
                triples: vec![[8, 9, 10]],
            }),
            unknown6: Some(11),
        }
    }

    fn actor(components: u32, state: Vec<Component>) -> Actor {
        Actor {
            version: 14,
            kingdom: 2,
            flags: 0x0800_0000,
            template_components: components,
            components,
            state: State::Decoded {
                components: state,
                trailer: trailer(),
            },
        }
    }

    fn body() -> Component {
        Component::Body(Body {
            version: 4,
            updatable: Some(Updatable { version: 1, id: 12 }),
            unknown: Some(-1.0),
            fraction: 0.5,
            unknown2: Some(0),
            unknown3: Some(1),
        })
    }

    fn mover() -> Component {
        Component::Mover(Mover {
            updatable: Updatable { version: 1, id: 13 },
            version: 8,
            unknown_vector: [1.0, 2.0, 3.0],
            unknown: [0.0, 1.0],
            fraction: Some(0.25),
            old: None,
            unknown2: [4.0, 5.0],
            old_flag: None,
            flag: Some(1),
            unknown3: Some(([6.0, 7.0, 8.0], 9.0)),
            flag2: Some((0, 2)),
            v6: Some(MoverV6 {
                flag: 1,
                a: [1.0; 3],
                flag2: 0,
                b: [2.0; 3],
                c: [3.0; 3],
            }),
        })
    }

    fn round_trip(actor: &Actor) -> Actor {
        let mut w = Writer::default();
        actor.write(&mut w).unwrap();
        let mut cur = Cursor::new(&w.0);
        let parsed = Actor::parse(&mut cur, WORLD).unwrap();
        assert!(cur.at_end());
        parsed
    }

    #[test]
    fn round_trips_typed_components() {
        let construction = Component::Construction(Construction {
            updatable: Updatable { version: 1, id: 11 },
            version: 1,
            progress: vec![1.0, 0.5],
            upgrade: Some(Upgrade {
                progress: vec![],
                template: "ger_factory_2".into(),
            }),
            unknown: None,
        });
        let property = |class, extra| Property {
            class,
            version: 3,
            unknown: 1,
            unknown2: Some(2),
            id: "tech_armor".into(),
            unknown3: Some((1, 3)),
            extra,
        };
        let properties = Component::Properties(Properties {
            updatable: Updatable { version: 1, id: 14 },
            version: 1,
            properties: vec![Some(property(0, None)), None, Some(property(1, Some(4)))],
        });
        // Economy: an updatable and a version.
        let economy = Component::Framed {
            bit: 9,
            data: [1u32, 15, 0].map(u32::to_le_bytes).concat(),
        };
        let components = 1 << 15 | 1 | 1 << 3 | 1 << 6 | 1 << 9;
        let a = actor(
            components,
            vec![construction, body(), mover(), properties, economy],
        );
        assert_eq!(round_trip(&a), a);
        assert_eq!(a.framed(9).map(<[u8]>::len), Some(12));
    }

    #[test]
    fn keeps_undecoded_state_whole() {
        // A projectile's layout isn't decoded.
        let mut w = Writer::default();
        let a = actor(1 << 1, vec![]);
        a.write(&mut w).unwrap();
        w.bytes(&[0xAA; 5]);
        let parsed = Actor::parse_whole(&mut Cursor::new(&w.0), WORLD).unwrap();
        assert!(matches!(&parsed.state, State::Raw(raw) if raw.len() == w.0.len() - 24));
        assert!(Actor::parse(&mut Cursor::new(&w.0), WORLD).is_err());
    }

    /// A sheep: body and mover only.
    fn member(id: u32) -> Vec<u8> {
        let mut w = Writer::default();
        Thing {
            template: "sheep_ewe".into(),
            class: super::super::ACTOR,
            id,
            name: String::new(),
            position: [1.0, 2.0, 3.0],
            facing: [0.0, -1.0, 0.0],
            flags: 0,
            scale: 1.0,
            variation: -1,
            unknown: 0,
            detail: Detail::Actor(actor(1 | 1 << 3, vec![body(), mover()])),
        }
        .write(&mut w)
        .unwrap();
        w.0
    }

    #[test]
    fn reads_organization_members() {
        let mut org = Writer::default();
        Updatable { version: 1, id: 20 }.write(&mut org);
        org.u32(10);
        org.u32(0);
        org.string("herd_ambient_large").unwrap();
        org.u32(2);
        org.string("sheep_ewe").unwrap();
        org.string("").unwrap();
        // Three positions, the middle one empty.
        org.u32(3);
        for (index, id) in [(0, Some(11)), (-1, None), (2, Some(12))] {
            org.i32(index);
            if let Some(id) = id {
                org.bytes(&member(id));
            }
        }
        (0..3).for_each(|_| org.string("sheep_ewe").unwrap());
        org.bytes(&[0; 16]);
        org.bytes(&[0; 3 * 12]);
        org.bytes(&[0; 6]);
        org.u32(0);
        org.u32(0);
        org.string("dmg_bullet").unwrap();
        org.bytes(&[0; 16]);
        org.bytes(&[0; 24]);
        let a = actor(
            1 << 7,
            vec![Component::Framed {
                bit: 7,
                data: org.0,
            }],
        );
        let parsed = round_trip(&a);
        assert_eq!(parsed, a);
        let org = parsed.organization(WORLD).unwrap();
        assert_eq!(org.layout, "herd_ambient_large");
        assert_eq!(org.slots, ["sheep_ewe", ""]);
        let ids: Vec<_> = org.members.iter().map(|m| m.id).collect();
        assert_eq!(ids, [11, 12]);
        let Detail::Actor(sheep) = &org.members[0].detail else {
            panic!("{:?}", org.members[0].detail)
        };
        assert_eq!(sheep.decoded().map(<[Component]>::len), Some(2));
    }
}
