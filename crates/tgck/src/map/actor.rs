//! Actors: the things made of components, such as units, buildings, herds and
//! kingdoms. Their layout is the `actor` type in [`rmp.ksy`].
//!
//! Components with a [`Component`] variant of their own decode into types;
//! the others are read to their end and kept as bytes ([`Component::Framed`]).
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use super::organization::{ElementState, Organization};
use crate::{Cursor, Error, Result, Writer};

/// The `actor` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Actor {
    pub version: u32,
    /// The owning [`Kingdom`](super::Kingdom)'s [`Thing::id`](super::Thing::id).
    pub kingdom: u32,
    pub states: u64,
    /// A bit per entry of [`COMPONENTS`].
    pub template_components: u32,
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
    /// The state and trailer as bytes, where `Actor::parse_whole` can't
    /// decode them.
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

/// An `updatable` of `rmp.ksy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Updatable {
    pub version: u32,
    pub id: i32,
}

/// The Construction component, the `construction` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Construction {
    pub updatable: Updatable,
    pub version: u32,
    pub progress: Vec<f32>,
    pub upgrade: Option<Upgrade>,
    pub old_upgrade: Option<[u32; 2]>,
}

/// A [`Construction`]'s `upgrade_progress` and `upgrade_template`.
#[derive(Debug, Clone, PartialEq)]
pub struct Upgrade {
    pub progress: Vec<f32>,
    pub template: String,
}

/// The Body component, the `body` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub version: u32,
    pub updatable: Option<Updatable>,
    pub death_time: Option<f32>,
    pub health: f32,
    pub sink_time: Option<f32>,
    pub last_damage_time: Option<f32>,
}

/// The Mover component, the `mover` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Mover {
    pub updatable: Updatable,
    pub version: u32,
    pub destination: [f32; 3],
    pub speed: f32,
    pub target_speed: f32,
    pub max_speed: Option<f32>,
    pub old: Option<[f32; 3]>,
    pub follow: u32,
    pub follow_distance: f32,
    pub old_flag: Option<u8>,
    pub turn_instantly: Option<u8>,
    pub target_facing: Option<[f32; 3]>,
    pub speed_scale: Option<f32>,
    /// `falling`, then `fall_drag`.
    pub falling: Option<(u8, f32)>,
    pub fall: Option<MoverFall>,
}

/// A [`Mover`]'s fields from `fall_turns` to `landing_offset`.
#[derive(Debug, Clone, PartialEq)]
pub struct MoverFall {
    pub turns: u8,
    pub direction: [f32; 3],
    pub has_landing: u8,
    pub landing: [f32; 3],
    pub landing_offset: [f32; 3],
}

/// The City component, the `city` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct City {
    pub updatable: Updatable,
    pub version: u32,
    pub buildings: Option<Vec<u32>>,
}

/// The Properties component, the `properties` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Properties {
    pub updatable: Updatable,
    pub version: u32,
    /// `None` where the record holds a class of -1.
    pub properties: Vec<Option<Property>>,
}

/// A `property` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub class: u32,
    pub version: u32,
    pub gained_at: f32,
    pub source: Option<u32>,
    pub id: String,
    pub duplicate: Option<u8>,
    pub from_actor: Option<u32>,
    /// Also `None` unless `class` is 1.
    pub extra: Option<u32>,
}

/// An `actor_trailer` of `rmp.ksy`. `saved_group` and `group` are
/// `Some(None)` where the version has them but their flag is clear.
#[derive(Debug, Clone, PartialEq)]
pub struct Trailer {
    pub main_component: i32,
    pub required_object: Option<u32>,
    pub old_stance: Option<u32>,
    pub saved_group: Option<Option<String>>,
    pub group: Option<Option<u32>>,
    pub map_string: Option<String>,
    /// `towing`, then `towed_by`.
    pub towing: Option<[u32; 2]>,
    pub number: Option<u32>,
    pub lists: Option<TrailerLists>,
    pub num_deploying: Option<u32>,
}

/// A [`Trailer`]'s fields from `engaged_by` to `recent_attacks`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrailerLists {
    /// `engaged_by`, `approached_by` and `attacked_by`.
    pub lists: [Vec<u32>; 3],
    /// Each `recent_attack`, its times as `f32` bits.
    pub recent_attacks: Vec<[u32; 3]>,
}

impl Actor {
    /// The first version that writes the states in 64 bits; earlier ones
    /// aren't decoded.
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
        let version = cur.version("actor", versions)?;
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
            version: cur.version("updatable", 0..=1)?,
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
        let version = cur.version("construction", 0..=1)?;
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
        w.f32_list("construction progress", &self.progress)?;
        let new = self.version >= 1;
        w.optional(
            new,
            "construction's upgrade",
            self.upgrade.as_ref(),
            |w, u| {
                w.f32_list("upgrade progress", &u.progress)?;
                w.string(&u.template)
            },
        )?;
        w.optional_plain(
            !new,
            "construction's old upgrade",
            self.old_upgrade,
            |w, old| old.iter().for_each(|&v| w.u32(v)),
        )
    }
}

impl City {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let updatable = Updatable::parse(cur)?;
        let version = cur.version("city", 0..=1)?;
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
        w.optional(self.version >= 1, "city's buildings", buildings, |w, v| {
            w.u32s("buildings", v)
        })
    }
}

impl Body {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = cur.version("body", 0..=4)?;
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
        w.optional_plain(v >= 2, "body's updatable", self.updatable, |w, u| {
            u.write(w)
        })?;
        w.optional_plain(v >= 1, "body's death time", self.death_time, |w, t| {
            w.f32(t)
        })?;
        w.f32(self.health);
        w.optional_plain(v >= 3, "body's sink time", self.sink_time, Writer::f32)?;
        w.optional_plain(
            v >= 4,
            "body's last damage time",
            self.last_damage_time,
            Writer::f32,
        )
    }
}

impl Mover {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let updatable = Updatable::parse(cur)?;
        let v = cur.version("mover", 0..=8)?;
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
        w.optional_plain(v >= 8, "mover's top speed", self.max_speed, Writer::f32)?;
        w.optional_plain(v < 4, "mover's old value", self.old, |w, o| w.f32s(&o))?;
        w.u32(self.follow);
        w.f32(self.follow_distance);
        let old_flag = (1..7).contains(&v);
        w.optional_plain(old_flag, "mover's old flag", self.old_flag, Writer::u8)?;
        w.optional_plain(
            v >= 2,
            "mover's instant turn",
            self.turn_instantly,
            Writer::u8,
        )?;
        w.optional_plain(
            v >= 3,
            "mover's target facing",
            self.target_facing,
            |w, f| w.f32s(&f),
        )?;
        w.optional_plain(v >= 3, "mover's speed scale", self.speed_scale, |w, s| {
            w.f32(s)
        })?;
        w.optional_plain(
            v >= 5,
            "mover's falling",
            self.falling,
            |w, (flag, drag)| {
                w.u8(flag);
                w.f32(drag);
            },
        )?;
        w.optional_plain(v >= 6, "mover's fall", self.fall.as_ref(), |w, fall| {
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
        let version = cur.version("properties", 0..=1)?;
        let properties = cur.list(|cur| match cur.u32()? {
            u32::MAX => Ok(None),
            class => Property::parse(cur, class).map(Some),
        })?;
        Ok(Properties {
            updatable,
            version,
            properties,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        self.updatable.write(w);
        w.u32(self.version);
        w.list("properties", &self.properties, |w, property| {
            match property {
                Some(p) => p.write(w)?,
                None => w.u32(u32::MAX),
            }
            Ok(())
        })?;
        Ok(())
    }
}

impl Property {
    /// Reads a property after its class.
    pub(super) fn parse(cur: &mut Cursor, class: u32) -> Result<Self> {
        if class > 1 {
            return Err(Error::Corrupt(format!("property class {class}")));
        }
        let version = cur.version("property", 0..=3)?;
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
                    cur.expect("property extra version", 0)?;
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
        w.optional_plain(v >= 1, "property's source", self.source, Writer::u32)?;
        w.string(&self.id)?;
        w.optional_plain(
            v >= 2,
            "property's duplicate flag",
            self.duplicate,
            Writer::u8,
        )?;
        w.optional_plain(v >= 2, "property's actor", self.from_actor, Writer::u32)?;
        let extra = self.class == 1 && v >= 3;
        w.optional_plain(extra, "property's extra value", self.extra, |w, e| {
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
                    cur.bool("saved group flag")?
                        .then(|| cur.string())
                        .transpose()
                })
                .transpose()?,
            group: (v >= 5)
                .then(|| cur.bool("group flag")?.then(|| cur.u32()).transpose())
                .transpose()?,
            map_string: (v >= 6).then(|| cur.string()).transpose()?,
            towing: (v >= 8)
                .then(|| Result::<_>::Ok([cur.u32()?, cur.u32()?]))
                .transpose()?,
            number: (v >= 10).then(|| cur.u32()).transpose()?,
            lists: (v >= 11)
                .then(|| {
                    let lists = [cur.u32s()?, cur.u32s()?, cur.u32s()?];
                    let recent_attacks = cur.list(|c| Ok([c.u32()?, c.u32()?, c.u32()?]))?;
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
        w.i32(self.main_component);
        w.optional_plain(v >= 2, "required object", self.required_object, Writer::u32)?;
        let old = (3..12).contains(&v);
        w.optional_plain(old, "old stance", self.old_stance, Writer::u32)?;
        let saved_group = self.saved_group.as_ref().map(Option::as_deref);
        w.optional(v >= 4, "saved group", saved_group, |w, s| {
            w.u8(s.is_some().into());
            s.map_or(Ok(()), |s| w.string(s))
        })?;
        w.optional_plain(v >= 5, "group", self.group, |w, g| {
            w.u8(g.is_some().into());
            g.into_iter().for_each(|g| w.u32(g));
        })?;
        w.optional(
            v >= 6,
            "map string",
            self.map_string.as_deref(),
            Writer::string,
        )?;
        w.optional_plain(v >= 8, "towing", self.towing, |w, t| {
            t.iter().for_each(|&id| w.u32(id));
        })?;
        w.optional_plain(v >= 10, "number", self.number, Writer::u32)?;
        w.optional(v >= 11, "trailer lists", self.lists.as_ref(), |w, lists| {
            for l in &lists.lists {
                w.u32s("trailer list", l)?;
            }
            w.list("recent attacks", &lists.recent_attacks, |w, attack| {
                attack.iter().for_each(|&v| w.u32(v));
                Ok(())
            })
        })?;
        w.optional_plain(v >= 13, "num deploying", self.num_deploying, Writer::u32)
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
                cur.expect("component version", 0).map(drop)
            }
            "Denizen" => skip_denizen(cur),
            "Supply" => skip_supply(cur),
            "Structure" => skip_structure(cur),
            "Recruit" => skip_recruit(cur),
            "Experience" => {
                cur.expect("experience version", 0)?;
                cur.u32().map(drop)
            }
            "MobileDeployment" => skip_mobile_deployment(cur),
            "Defender" => skip_defender(cur),
            "Research" => {
                Updatable::parse(cur)?;
                cur.expect("research version", 0)?;
                skip_strings(cur)
            }
            "Transport" => skip_transport(cur),
            "Lair" => {
                Updatable::parse(cur)?;
                cur.expect("lair version", 0)?;
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
    cur.expect("layout version", 0)?;
    cur.string()?;
    skip_strings(cur)
}

fn skip_ability(cur: &mut Cursor) -> Result<()> {
    let v = cur.version("ability", 0..=1)?;
    cur.bytes(13)?;
    if v >= 1 {
        cur.u32s()?;
    }
    Ok(())
}

fn skip_character(cur: &mut Cursor) -> Result<()> {
    Updatable::parse(cur)?;
    let v = cur.version("character", 0..=2)?;
    cur.u32()?;
    if v < 1 {
        cur.u32()?;
    }
    // The animation type and index, then its player's state.
    cur.bytes(8)?;
    cur.expect("animation version", 0)?;
    cur.bytes(13)?;
    if v >= 2 {
        cur.u32()?;
    }
    Ok(())
}

fn skip_performer(cur: &mut Cursor) -> Result<()> {
    Updatable::parse(cur)?;
    let v = cur.version("performer", 4..=9)?;
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
    let v = cur.version("denizen", 0..=4)?;
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
    let v = cur.version("supply", 0..=2)?;
    if v >= 1 {
        Updatable::parse(cur)?;
    }
    if v >= 2 {
        cur.bytes(8)?;
    }
    Ok(())
}

fn skip_structure(cur: &mut Cursor) -> Result<()> {
    let v = cur.version("structure", 0..=13)?;
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
    let v = cur.version("recruit", 0..=3)?;
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
    let v = cur.version("mobile deployment", 0..=3)?;
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
    cur.expect("id list version", 0)?;
    cur.u32s().map(drop)
}

fn skip_resource_amounts(cur: &mut Cursor) -> Result<()> {
    cur.expect("resource amounts version", 0)?;
    cur.list(Cursor::f32).map(drop)
}

fn skip_defender(cur: &mut Cursor) -> Result<()> {
    Updatable::parse(cur)?;
    let v = cur.version("defender", 0..=2)?;
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
    let v = cur.version("transport", 0..=6)?;
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
    let v = cur.version("hq", 0..=3)?;
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
    let v = cur.version("mind", 0..=3)?;
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
    let v = cur.version("command", 0..=3)?;
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
    cur.expect("path version", 0)?;
    let n = cur.u32()?;
    bytes(cur, n, 36)?;
    cur.u32().map(drop)
}

fn skip_element_mind(cur: &mut Cursor) -> Result<()> {
    let v = cur.version("element mind", 0..=14)?;
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
    let v = cur.version("organization mind", 0..=21)?;
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
        let mut cur = Cursor::new(data);
        Updatable::parse(&mut cur).ok()?;
        let _version = cur.u32().ok()?;
        cur.i32().ok()
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
            } => Updatable::parse(&mut Cursor::new(data)).ok(),
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
    fn reads_a_defender() {
        let mut defender = Writer::default();
        Updatable { version: 1, id: 20 }.write(&mut defender);
        defender.u32(0);
        let w = hollow(1 << DEFENDER);
        let trailer_at = 24;
        let bytes = [&w.0[..trailer_at], &defender.0, &w.0[trailer_at..]].concat();
        let parsed = Actor::parse(&mut Cursor::new(&bytes), WORLD).unwrap();
        assert_eq!(parsed.defender(), Some(Updatable { version: 1, id: 20 }));
        assert_eq!(actor(0, vec![]).defender(), None);
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
