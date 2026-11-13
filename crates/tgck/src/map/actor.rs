//! Actors: the things made of components, such as units, buildings, herds and
//! kingdoms. Their layout is the `actor` type in [`rmp.ksy`].
//!
//! Components with a [`Component`] variant of their own decode into types;
//! the others are read to their end and kept as bytes ([`Component::Framed`]).
//! A record whose state doesn't decode to its exact end is kept whole
//! ([`State::Raw`]), which only a record of known size allows.
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use super::expect;
use super::header::{bool, check_version, optional, optional_plain};
use super::organization::{ElementState, Organization};
use crate::{Cursor, Error, Result, Writer};

/// A thing made of components, such as a unit or building.
#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    pub version: u32,
    /// The owning [`Kingdom`](super::Kingdom)'s [`Thing::id`](super::Thing::id); 0 for none.
    pub kingdom: u32,
    /// The states it's in, a bit each (see `actor_state` in `rmp.ksy`).
    pub states: u64,
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
    Organization(Organization),
    Element(ElementState),
    City(City),
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
const DEFENDER: u32 = 20;
const AIRCRAFT: u32 = 23;
const CITY: u32 = 29;
const HQ: u32 = 30;

/// The components whose state is decoded, not kept as its bytes.
const DECODED: [u32; 7] = [
    CONSTRUCTION,
    BODY,
    MOVER,
    PROPERTIES,
    ORGANIZATION,
    ELEMENT,
    CITY,
];

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
    /// Before version 1: an upgrade under the old rules, which the game
    /// refuses unless both are 0.
    pub old_upgrade: Option<[u32; 2]>,
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
    /// From version 1 on: when it died; -1 while alive.
    pub death_time: Option<f32>,
    /// Its health as a fraction of the template's `health`.
    pub health: f32,
    /// From version 3 on: how long its remains take to sink, set when it dies.
    pub sink_time: Option<f32>,
    /// From version 4 on: when it last took damage.
    pub last_damage_time: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mover {
    pub updatable: Updatable,
    pub version: u32,
    pub destination: [f32; 3],
    pub speed: f32,
    /// The speed it's accelerating or braking towards.
    pub target_speed: f32,
    /// From version 8 on: its top speed as a fraction of the template's
    /// `speed_max`.
    pub max_speed: Option<f32>,
    /// Before version 4, and ignored.
    pub old: Option<[f32; 3]>,
    /// The thing id of an actor it's following.
    pub follow: u32,
    /// How close it keeps to the actor it follows; junk when it follows none.
    pub follow_distance: f32,
    /// Versions 1 to 6.
    pub old_flag: Option<u8>,
    /// From version 2 on: whether it snaps to its target facing.
    pub turn_instantly: Option<u8>,
    /// From version 3 on.
    pub target_facing: Option<[f32; 3]>,
    /// From version 3 on: a factor on how far it moves for its speed.
    pub speed_scale: Option<f32>,
    /// From version 5 on: whether it's falling or sliding along a set
    /// course (marked in the actor's flags, too), and the fall's drag.
    pub falling: Option<(u8, f32)>,
    /// From version 6 on.
    pub fall: Option<MoverFall>,
}

/// The course of a fall.
#[derive(Debug, Clone, PartialEq)]
pub struct MoverFall {
    /// Whether it turns to face the way it falls.
    pub turns: u8,
    pub direction: [f32; 3],
    /// Whether the fall ends at `landing`.
    pub has_landing: u8,
    pub landing: [f32; 3],
    /// From where the fall started to `landing`.
    pub landing_offset: [f32; 3],
}

/// A city's centre, which stands for the buildings around it.
#[derive(Debug, Clone, PartialEq)]
pub struct City {
    pub updatable: Updatable,
    pub version: u32,
    /// From version 1 on: the thing ids of the buildings that make it up.
    pub buildings: Option<Vec<u32>>,
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
    /// The game time the actor gained it.
    pub gained_at: f32,
    /// From version 1 on: where it came from, e.g. 1 an ability's effect, 2
    /// granted kingdom-wide (technology and the like), 5 the terrain the
    /// actor stands on, 9 its stance.
    pub source: Option<u32>,
    /// The shared property data's id.
    pub id: String,
    /// From version 2 on: set when the actor already held it from the same
    /// source, so this copy adds nothing.
    pub duplicate: Option<u8>,
    /// From version 2 on: the thing id of the actor it came from; 0 for none.
    pub from_actor: Option<u32>,
    /// Class 1 from version 3 on: a second actor's thing id, set when an
    /// ability hands the property over.
    pub extra: Option<u32>,
}

/// What follows an actor's components. Thing ids are 0 for none.
#[derive(Debug, Clone, PartialEq)]
pub struct Trailer {
    /// The bit of the component that's in charge, or -1.
    pub main_component: i32,
    /// From version 2 on: the object it needs alongside it.
    pub required_object: Option<u32>,
    /// Versions 3 to 11, which the game ignores.
    pub old_stance: Option<u32>,
    /// From version 4 on: the name of its kingdom's saved group it's in.
    pub saved_group: Option<Option<String>>,
    /// From version 5 on: the group thing moving it with others.
    pub group: Option<Option<u32>>,
    /// From version 6 on: the MAPSTRINGS key of its own name.
    pub map_string: Option<String>,
    /// From version 8 on: the trailer it tows and what tows it.
    pub towing: Option<[u32; 2]>,
    /// From version 10 on: the ordinal in its name, as in 3rd Infantry.
    pub number: Option<u32>,
    /// From version 11 on.
    pub lists: Option<TrailerLists>,
    /// From version 13 on: how many units are deploying onto it.
    pub num_deploying: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrailerLists {
    /// The organizations engaging it, the units moving to it and the
    /// actors that have attacked it.
    pub lists: [Vec<u32>; 3],
    /// Recent attacks: the attacker, then the time of its latest hit and
    /// how long that counts, as `f32` bits.
    pub recent_attacks: Vec<[u32; 3]>,
}

impl Actor {
    /// The first version that writes the states in 64 bits; the layout of
    /// earlier ones is unconfirmed.
    const MIN_VERSION: u32 = 9;
    /// The newest version the game reads.
    const MAX_VERSION: u32 = 14;

    /// Reads an actor whose end is known: its state is kept raw when it
    /// doesn't decode to exactly that end.
    pub(super) fn parse_whole(cur: &mut Cursor, world: u32) -> Result<Self> {
        let mut actor = Actor::parse_header(cur)?;
        let start = cur.pos();
        let decoded = actor.parse_state(cur, world, 0);
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
        Actor::parse_nested(cur, world, 0)
    }

    /// As [`Actor::parse`], for an actor `depth` organizations down.
    pub(super) fn parse_nested(cur: &mut Cursor, world: u32, depth: u32) -> Result<Self> {
        let mut actor = Actor::parse_header(cur)?;
        actor.state = actor.parse_state(cur, world, depth)?;
        Ok(actor)
    }

    fn parse_header(cur: &mut Cursor) -> Result<Self> {
        let versions = Actor::MIN_VERSION..=Actor::MAX_VERSION;
        let version = check_version("actor", cur.u32()?, versions)?;
        Ok(Actor {
            version,
            kingdom: cur.u32()?,
            states: cur.u64()?,
            template_components: cur.u32()?,
            components: cur.u32()?,
            state: State::Raw(Vec::new()),
        })
    }

    fn parse_state(&self, cur: &mut Cursor, world: u32, depth: u32) -> Result<State> {
        // A component bit no class has would leave its record unread.
        if (self.components | self.template_components) >> COMPONENTS.len() != 0 {
            return Err(Error::Corrupt(format!(
                "actor components {:#x} of {:#x}",
                self.components, self.template_components
            )));
        }
        let components = record_order(self.components)
            .map(|bit| self.parse_component(cur, bit, world, depth))
            .collect::<Result<_>>()?;
        Ok(State::Decoded {
            components,
            trailer: Trailer::parse(cur, self.version)?,
        })
    }

    fn parse_component(
        &self,
        cur: &mut Cursor,
        bit: u32,
        world: u32,
        depth: u32,
    ) -> Result<Component> {
        Ok(match bit {
            CONSTRUCTION => Component::Construction(Construction::parse(cur)?),
            BODY => Component::Body(Body::parse(cur)?),
            MOVER => Component::Mover(Mover::parse(cur)?),
            PROPERTIES => Component::Properties(Properties::parse(cur)?),
            ORGANIZATION => Component::Organization(Organization::parse_nested(cur, world, depth)?),
            ELEMENT => Component::Element(ElementState::parse(cur, world)?),
            CITY => Component::City(City::parse(cur)?),
            _ => {
                let start = cur.pos();
                let skip = Skip {
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
        w.u64(self.states);
        w.u32(self.template_components);
        w.u32(self.components);
        match &self.state {
            State::Decoded {
                components,
                trailer,
            } => {
                // What reading refuses or would read as something else.
                let unknown = (self.components | self.template_components) >> COMPONENTS.len();
                let miscast = components
                    .iter()
                    .any(|c| matches!(c, Component::Framed { bit, .. } if DECODED.contains(bit)));
                let bits = components.iter().map(Component::bit);
                if unknown != 0 || miscast || !bits.eq(record_order(self.components)) {
                    return Err(Error::Unwritable(format!(
                        "actor's components aren't those of its bits {:#x}, in record order",
                        self.components
                    )));
                }
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

pub(super) fn version(cur: &mut Cursor, what: &str, max: u32) -> Result<u32> {
    check_version(what, cur.u32()?, 0..=max)
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
    /// Its bit in [`COMPONENTS`].
    fn bit(&self) -> u32 {
        match self {
            Component::Construction(_) => CONSTRUCTION,
            Component::Body(_) => BODY,
            Component::Mover(_) => MOVER,
            Component::Properties(_) => PROPERTIES,
            Component::Organization(_) => ORGANIZATION,
            Component::Element(_) => ELEMENT,
            Component::City(_) => CITY,
            Component::Framed { bit, .. } => *bit,
        }
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        match self {
            Component::Construction(c) => c.write(w),
            Component::Body(b) => b.write(w),
            Component::Mover(m) => m.write(w),
            Component::Properties(p) => p.write(w),
            Component::Organization(o) => o.write(w),
            Component::Element(e) => e.write(w),
            Component::City(c) => c.write(w),
            Component::Framed { data, .. } => {
                w.bytes(data);
                Ok(())
            }
        }
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
        let progress = cur.list(Cursor::f32)?;
        let (upgrade, old_upgrade) = if version >= 1 {
            let progress = cur.list(Cursor::f32)?;
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
            old_upgrade,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        self.updatable.write(w);
        w.u32(self.version);
        write_f32s(w, &self.progress);
        let new = self.version >= 1;
        optional(new, "construction's upgrade", self.upgrade.as_ref(), |u| {
            write_f32s(w, &u.progress);
            w.string(&u.template)
        })?;
        optional_plain(
            !new,
            "construction's old upgrade",
            self.old_upgrade,
            |old| old.iter().for_each(|&v| w.u32(v)),
        )
    }
}

impl City {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let updatable = Updatable::parse(cur)?;
        let version = version(cur, "city", 1)?;
        Ok(City {
            updatable,
            version,
            buildings: (version >= 1).then(|| cur.u32s()).transpose()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        self.updatable.write(w);
        w.u32(self.version);
        let buildings = self.buildings.as_deref();
        optional_plain(self.version >= 1, "city's buildings", buildings, |b| {
            w.u32s(b)
        })
    }
}

impl Body {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = version(cur, "body", 4)?;
        Ok(Body {
            version,
            updatable: (version >= 2).then(|| Updatable::parse(cur)).transpose()?,
            death_time: (version >= 1).then(|| cur.f32()).transpose()?,
            health: cur.f32()?,
            sink_time: (version >= 3).then(|| cur.f32()).transpose()?,
            last_damage_time: (version >= 4).then(|| cur.f32()).transpose()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        let v = self.version;
        w.u32(v);
        optional_plain(v >= 2, "body's updatable", self.updatable, |u| u.write(w))?;
        optional_plain(v >= 1, "body's death time", self.death_time, |t| w.f32(t))?;
        w.f32(self.health);
        optional_plain(v >= 3, "body's sink time", self.sink_time, |t| w.f32(t))?;
        optional_plain(
            v >= 4,
            "body's last damage time",
            self.last_damage_time,
            |t| w.f32(t),
        )
    }
}

impl Mover {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let updatable = Updatable::parse(cur)?;
        let v = version(cur, "mover", 8)?;
        Ok(Mover {
            updatable,
            version: v,
            destination: cur.f32s()?,
            speed: cur.f32()?,
            target_speed: cur.f32()?,
            max_speed: (v >= 8).then(|| cur.f32()).transpose()?,
            old: (v < 4).then(|| cur.f32s()).transpose()?,
            follow: cur.u32()?,
            follow_distance: cur.f32()?,
            old_flag: (1..7).contains(&v).then(|| cur.u8()).transpose()?,
            turn_instantly: (v >= 2).then(|| cur.u8()).transpose()?,
            target_facing: (v >= 3).then(|| cur.f32s()).transpose()?,
            speed_scale: (v >= 3).then(|| cur.f32()).transpose()?,
            falling: (v >= 5)
                .then(|| Result::<_>::Ok((cur.u8()?, cur.f32()?)))
                .transpose()?,
            fall: (v >= 6)
                .then(|| {
                    Result::<_>::Ok(MoverFall {
                        turns: cur.u8()?,
                        direction: cur.f32s()?,
                        has_landing: cur.u8()?,
                        landing: cur.f32s()?,
                        landing_offset: cur.f32s()?,
                    })
                })
                .transpose()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        let v = self.version;
        self.updatable.write(w);
        w.u32(v);
        w.f32s(&self.destination);
        w.f32(self.speed);
        w.f32(self.target_speed);
        optional_plain(v >= 8, "mover's top speed", self.max_speed, |s| w.f32(s))?;
        optional_plain(v < 4, "mover's old value", self.old, |o| w.f32s(&o))?;
        w.u32(self.follow);
        w.f32(self.follow_distance);
        let old_flag = (1..7).contains(&v);
        optional_plain(old_flag, "mover's old flag", self.old_flag, |f| w.u8(f))?;
        optional_plain(v >= 2, "mover's instant turn", self.turn_instantly, |t| {
            w.u8(t)
        })?;
        optional_plain(v >= 3, "mover's target facing", self.target_facing, |f| {
            w.f32s(&f)
        })?;
        optional_plain(v >= 3, "mover's speed scale", self.speed_scale, |s| {
            w.f32(s)
        })?;
        optional_plain(v >= 5, "mover's falling", self.falling, |(flag, drag)| {
            w.u8(flag);
            w.f32(drag);
        })?;
        optional_plain(v >= 6, "mover's fall", self.fall.as_ref(), |fall| {
            w.u8(fall.turns);
            w.f32s(&fall.direction);
            w.u8(fall.has_landing);
            w.f32s(&fall.landing);
            w.f32s(&fall.landing_offset);
        })
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
            gained_at: cur.f32()?,
            source: (version >= 1).then(|| cur.u32()).transpose()?,
            id: cur.string()?,
            duplicate: (version >= 2).then(|| cur.u8()).transpose()?,
            from_actor: (version >= 2).then(|| cur.u32()).transpose()?,
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
        if self.class > 1 {
            return Err(Error::Unwritable(format!("property class {}", self.class)));
        }
        let v = self.version;
        w.u32(v);
        w.f32(self.gained_at);
        optional_plain(v >= 1, "property's source", self.source, |s| w.u32(s))?;
        w.string(&self.id)?;
        optional_plain(v >= 2, "property's duplicate flag", self.duplicate, |d| {
            w.u8(d)
        })?;
        optional_plain(v >= 2, "property's actor", self.from_actor, |a| w.u32(a))?;
        let extra = self.class == 1 && v >= 3;
        optional_plain(extra, "property's extra value", self.extra, |e| {
            w.u32(0);
            w.u32(e);
        })
    }
}

impl Trailer {
    fn parse(cur: &mut Cursor, v: u32) -> Result<Self> {
        Ok(Trailer {
            main_component: cur.i32()?,
            required_object: (v >= 2).then(|| cur.u32()).transpose()?,
            old_stance: (3..12).contains(&v).then(|| cur.u32()).transpose()?,
            saved_group: (v >= 4)
                .then(|| {
                    bool(cur, "saved group flag")?
                        .then(|| cur.string())
                        .transpose()
                })
                .transpose()?,
            group: (v >= 5)
                .then(|| bool(cur, "group flag")?.then(|| cur.u32()).transpose())
                .transpose()?,
            map_string: (v >= 6).then(|| cur.string()).transpose()?,
            towing: (v >= 8)
                .then(|| Result::<_>::Ok([cur.u32()?, cur.u32()?]))
                .transpose()?,
            number: (v >= 10).then(|| cur.u32()).transpose()?,
            lists: (v >= 11)
                .then(|| {
                    let lists = [cur.u32s()?, cur.u32s()?, cur.u32s()?];
                    let n = cur.u32()?;
                    let recent_attacks = bytes(cur, n, 12)?
                        .chunks(12)
                        .map(|c| {
                            let [a, b, c] = [0, 4, 8]
                                .map(|i| u32::from_le_bytes(c[i..i + 4].try_into().unwrap()));
                            [a, b, c]
                        })
                        .collect();
                    Result::<_>::Ok(TrailerLists {
                        lists,
                        recent_attacks,
                    })
                })
                .transpose()?,
            num_deploying: (v >= 13).then(|| cur.u32()).transpose()?,
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
        has(self.required_object.is_some(), v >= 2)?;
        has(self.old_stance.is_some(), (3..12).contains(&v))?;
        has(self.saved_group.is_some(), v >= 4)?;
        has(self.group.is_some(), v >= 5)?;
        has(self.map_string.is_some(), v >= 6)?;
        has(self.towing.is_some(), v >= 8)?;
        has(self.number.is_some(), v >= 10)?;
        has(self.lists.is_some(), v >= 11)?;
        has(self.num_deploying.is_some(), v >= 13)?;
        w.i32(self.main_component);
        for v in [self.required_object, self.old_stance]
            .into_iter()
            .flatten()
        {
            w.u32(v);
        }
        if let Some(s) = &self.saved_group {
            w.u8(s.is_some().into());
            if let Some(s) = s {
                w.string(s)?;
            }
        }
        if let Some(v) = self.group {
            w.u8(v.is_some().into());
            if let Some(v) = v {
                w.u32(v);
            }
        }
        if let Some(s) = &self.map_string {
            w.string(s)?;
        }
        if let Some(vs) = self.towing {
            vs.iter().for_each(|&v| w.u32(v));
        }
        if let Some(v) = self.number {
            w.u32(v);
        }
        if let Some(lists) = &self.lists {
            lists.lists.iter().for_each(|l| w.u32s(l));
            w.u32(lists.recent_attacks.len() as u32);
            lists
                .recent_attacks
                .iter()
                .flatten()
                .for_each(|&v| w.u32(v));
        }
        if let Some(v) = self.num_deploying {
            w.u32(v);
        }
        Ok(())
    }
}

/// Reads past the state of components not decoded into types, to find
/// where it ends.
struct Skip {
    template_components: u32,
}

impl Skip {
    fn component(&self, cur: &mut Cursor, bit: u32) -> Result<()> {
        match COMPONENTS[bit as usize] {
            "Character" => skip_character(cur),
            "Mind" => self.mind(cur),
            "Performer" => skip_performer(cur),
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
                cur.u32s()?;
                cur.bytes(8).map(drop)
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
        cur.u32s()?;
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
    let v = check_version("performer", cur.u32()?, 4..=9)?;
    cur.bytes(24)?;
    if v < 5 {
        cur.u32s()?;
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
        cur.u32s()?;
        cur.u32s()?;
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
    cur.u32s().map(drop)
}

fn skip_resource_amounts(cur: &mut Cursor) -> Result<()> {
    version(cur, "resource amounts", 0)?;
    cur.list(Cursor::f32).map(drop)
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
        cur.u32s()?;
    }
    if v >= 4 {
        cur.u32()?;
    }
    cur.u32()?;
    // The cargo's cost and upkeep.
    skip_resource_amounts(cur)?;
    skip_resource_amounts(cur)?;
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
        1 => drop(cur.u32s()?),
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
    cur.u32s()?;
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
    cur.bytes(12)?;
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
        if v == 4 || v >= 13 {
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
        cur.u32s()?;
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

impl Actor {
    /// The [`Thing::id`](super::Thing::id) of the trailer it tows, if it
    /// tows one.
    pub fn towed(&self) -> Option<u32> {
        match &self.state {
            State::Decoded { trailer, .. } => trailer.towing.map(|[t, _]| t).filter(|&t| t != 0),
            State::Raw(_) => None,
        }
    }

    /// The herd or regiment this actor is, if its record holds one.
    pub fn organization(&self) -> Option<&Organization> {
        self.decoded()?.iter().find_map(|c| match c {
            Component::Organization(o) => Some(o),
            _ => None,
        })
    }

    /// Which of its nation's division badges this HQ's regiments wear, if
    /// it's an HQ; -1 for none.
    pub fn hq_badge(&self) -> Option<i32> {
        let data = self.decoded()?.iter().find_map(|c| match c {
            Component::Framed { bit: HQ, data } => Some(data),
            _ => None,
        })?;
        // After the updatable header and the component's version.
        let badge = data.get(12..16)?;
        Some(i32::from_le_bytes(badge.try_into().ok()?))
    }

    pub fn body(&self) -> Option<&Body> {
        self.decoded()?.iter().find_map(|c| match c {
            Component::Body(b) => Some(b),
            _ => None,
        })
    }

    /// The city this actor is the centre of, if its record holds one.
    pub fn city(&self) -> Option<&City> {
        self.decoded()?.iter().find_map(|c| match c {
            Component::City(c) => Some(c),
            _ => None,
        })
    }

    /// Its defender's place among the world's updatable objects, if its
    /// record holds a defender: -1 while it's idle.
    pub fn defender(&self) -> Option<Updatable> {
        self.decoded()?.iter().find_map(|c| match c {
            Component::Framed {
                bit: DEFENDER,
                data,
            } => Some(Updatable {
                version: u32::from_le_bytes(data.get(0..4)?.try_into().ok()?),
                id: i32::from_le_bytes(data.get(4..8)?.try_into().ok()?),
            }),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{DamageTail, Detail, Thing};

    const WORLD: u32 = 28;

    fn trailer() -> Trailer {
        Trailer {
            main_component: 3,
            required_object: Some(1),
            old_stance: None,
            saved_group: Some(Some("order".into())),
            group: Some(None),
            map_string: Some(String::new()),
            towing: Some([2, 3]),
            number: Some(4),
            lists: Some(TrailerLists {
                lists: [vec![5], vec![], vec![6, 7]],
                recent_attacks: vec![[8, 9, 10]],
            }),
            num_deploying: Some(11),
        }
    }

    fn actor(components: u32, state: Vec<Component>) -> Actor {
        Actor {
            version: 14,
            kingdom: 2,
            states: 0x0800_0000,
            template_components: components,
            components,
            state: State::Decoded {
                components: state,
                trailer: trailer(),
            },
        }
    }

    /// An actor's record with `components` set but their state left out,
    /// for a test to splice in after the 24-byte header.
    fn hollow(components: u32) -> Writer {
        let mut w = Writer::default();
        actor(0, vec![]).write(&mut w).unwrap();
        for at in [16, 20] {
            w.0[at..at + 4].copy_from_slice(&components.to_le_bytes());
        }
        w
    }

    fn body() -> Component {
        Component::Body(Body {
            version: 4,
            updatable: Some(Updatable { version: 1, id: 12 }),
            death_time: Some(-1.0),
            health: 0.5,
            sink_time: Some(0.0),
            last_damage_time: Some(-999.0),
        })
    }

    fn mover() -> Component {
        Component::Mover(Mover {
            updatable: Updatable { version: 1, id: 13 },
            version: 8,
            destination: [1.0, 2.0, 3.0],
            speed: 0.0,
            target_speed: 1.0,
            max_speed: Some(0.25),
            old: None,
            follow: 4,
            follow_distance: 5.0,
            old_flag: None,
            turn_instantly: Some(1),
            target_facing: Some([6.0, 7.0, 8.0]),
            speed_scale: Some(9.0),
            falling: Some((0, 2.0)),
            fall: Some(MoverFall {
                turns: 1,
                direction: [1.0; 3],
                has_landing: 0,
                landing: [2.0; 3],
                landing_offset: [3.0; 3],
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
            old_upgrade: None,
        });
        let property = |class, extra| Property {
            class,
            version: 3,
            gained_at: 1.0,
            source: Some(2),
            id: "tech_armor".into(),
            duplicate: Some(1),
            from_actor: Some(3),
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
        let city = Component::City(City {
            updatable: Updatable { version: 1, id: 16 },
            version: 1,
            buildings: Some(vec![30, 31]),
        });
        let components = 1 << 15 | 1 | 1 << 3 | 1 << 6 | 1 << 9 | 1 << 29;
        let a = actor(
            components,
            vec![construction, body(), mover(), properties, economy, city],
        );
        assert_eq!(round_trip(&a), a);
    }

    #[test]
    fn refuses_components_that_dont_suit() {
        let write = |a: Actor| a.write(&mut Writer::default());
        assert!(write(actor(1, vec![body()])).is_ok());
        assert!(write(actor(1, vec![])).is_err(), "one missing");
        let both = 1 | 1 << MOVER;
        assert!(write(actor(both, vec![mover(), body()])).is_err(), "order");

        let Component::Body(mut old) = body() else {
            unreachable!()
        };
        old.version = 3;
        let old = Component::Body(old);
        assert!(write(actor(1, vec![old])).is_err(), "a later field");
    }

    #[test]
    fn refuses_components_reading_wouldnt_take() {
        let write = |a: Actor| a.write(&mut Writer::default());
        let mut unknown = actor(1, vec![body()]);
        unknown.components |= 1 << 31;
        assert!(write(unknown).is_err(), "a bit no component has");
        let framed = Component::Framed {
            bit: BODY,
            data: Vec::new(),
        };
        assert!(
            write(actor(1, vec![framed])).is_err(),
            "a decoded one's bytes"
        );
    }

    #[test]
    fn writes_old_versions_as_they_are_read() {
        let updatable = Updatable { version: 1, id: 1 };
        let construction = Construction {
            updatable,
            version: 0,
            progress: vec![0.5],
            upgrade: None,
            old_upgrade: Some([1, 2]),
        };
        let mut w = Writer::default();
        construction.write(&mut w).unwrap();
        let read = Construction::parse(&mut Cursor::new(&w.0)).unwrap();
        assert_eq!(read, construction);
        let newer = Construction {
            version: 1,
            ..construction
        };
        assert!(newer.write(&mut Writer::default()).is_err());

        let city = City {
            updatable,
            version: 0,
            buildings: None,
        };
        let mut w = Writer::default();
        city.write(&mut w).unwrap();
        assert_eq!(City::parse(&mut Cursor::new(&w.0)).unwrap(), city);
        let newer = City { version: 1, ..city };
        assert!(newer.write(&mut Writer::default()).is_err());

        let Component::Mover(newest) = mover() else {
            unreachable!()
        };
        let olds = [
            Mover {
                version: 3,
                max_speed: None,
                old: Some([1.0; 3]),
                old_flag: Some(1),
                falling: None,
                fall: None,
                ..newest.clone()
            },
            Mover {
                version: 6,
                max_speed: None,
                old_flag: Some(1),
                ..newest.clone()
            },
            Mover {
                version: 0,
                max_speed: None,
                old: Some([1.0; 3]),
                turn_instantly: None,
                target_facing: None,
                speed_scale: None,
                falling: None,
                fall: None,
                ..newest.clone()
            },
        ];
        for old in olds {
            let mut w = Writer::default();
            old.write(&mut w).unwrap();
            assert_eq!(Mover::parse(&mut Cursor::new(&w.0)).unwrap(), old);
            let newer = Mover {
                version: 8,
                ..old.clone()
            };
            assert!(newer.write(&mut Writer::default()).is_err());
        }

        let property = Property {
            class: 1,
            version: 2,
            gained_at: 1.0,
            source: Some(2),
            id: "tech_armor".into(),
            duplicate: Some(1),
            from_actor: Some(3),
            extra: None,
        };
        let mut w = Writer::default();
        property.write_body(&mut w).unwrap();
        assert_eq!(
            Property::parse(&mut Cursor::new(&w.0), 1).unwrap(),
            property
        );
        for (class, version) in [(1, 3), (1, 0), (2, 2)] {
            let other = Property {
                class,
                version,
                ..property.clone()
            };
            assert!(other.write_body(&mut Writer::default()).is_err());
        }
    }

    #[test]
    fn keeps_undecoded_state_whole() {
        // A projectile's layout isn't decoded.
        let mut w = hollow(1 << 1);
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
            seen_by: 0,
            detail: Detail::Actor(actor(1 | 1 << 3, vec![body(), mover()])),
        }
        .write(&mut w, super::super::ReadAs::Actor(super::super::ACTOR))
        .unwrap();
        w.0
    }

    #[test]
    fn reads_an_hq_badge() {
        let mut hq = Writer::default();
        Updatable { version: 1, id: 20 }.write(&mut hq);
        hq.u32(3);
        hq.i32(7);
        // No attachments.
        hq.u32(0);
        let w = hollow(1 << HQ);
        let trailer_at = 24;
        let bytes = [&w.0[..trailer_at], &hq.0, &w.0[trailer_at..]].concat();
        let parsed = Actor::parse(&mut Cursor::new(&bytes), WORLD).unwrap();
        assert_eq!(parsed.hq_badge(), Some(7));
        assert_eq!(actor(0, vec![]).hq_badge(), None);
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
        let w = hollow(1 << 7);
        // The organization goes between the header and the trailer.
        let trailer_at = 24;
        let bytes = [&w.0[..trailer_at], &org.0, &w.0[trailer_at..]].concat();
        let parsed = Actor::parse(&mut Cursor::new(&bytes), WORLD).unwrap();
        let mut again = Writer::default();
        parsed.write(&mut again).unwrap();
        assert_eq!(again.0, bytes, "writes back what it read");
        let org = parsed.organization().unwrap();
        assert_eq!(org.layout, "herd_ambient_large");
        assert_eq!(org.slots, ["sheep_ewe", ""]);
        let ids: Vec<_> = org.members.iter().map(|m| m.id).collect();
        assert_eq!(ids, [11, 12]);
        let Detail::Actor(sheep) = &org.members[0].detail else {
            panic!("{:?}", org.members[0].detail)
        };
        assert_eq!(sheep.decoded().map(<[Component]>::len), Some(2));
        let filled: Vec<_> = org.positions.iter().map(|p| p.index).collect();
        assert_eq!(filled, [0, -1, 2]);
        assert_eq!(
            org.fighting.as_ref().map(|f| f.damage.tail),
            Some(DamageTail::Words([0, 0]))
        );

        let mut missing = org.clone();
        missing.members.pop();
        assert!(missing.write(&mut Writer::default()).is_err());

        // Members read back only as actors of the actor class.
        let area = Detail::Area(super::super::Area {
            version: 2,
            id: None,
            name: String::new(),
            shape: 1,
            radius: 1.0,
            bounds: [0.0; 4],
            inside: Vec::new(),
        });
        for (class, detail) in [(super::super::AREA, area), (99, Detail::Raw(Vec::new()))] {
            let mut wrong = org.clone();
            (wrong.members[0].class, wrong.members[0].detail) = (class, detail);
            let err = wrong.write(&mut Writer::default());
            assert!(matches!(err, Err(Error::Unwritable(_))), "{err:?}");
        }
    }
}
