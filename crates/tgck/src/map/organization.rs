//! The Organization and Element components: a herd or regiment holding its
//! members' whole records, and what each member keeps of the damage it
//! deals. Laid out as `organization` and `element` in [`rmp.ksy`].
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use super::actor::{Updatable, version};
use super::header::optional;
use super::{Thing, expect};
use crate::{Cursor, Error, Result, Writer};

/// A herd or regiment.
#[derive(Debug, Clone, PartialEq)]
pub struct Organization {
    pub updatable: Updatable,
    pub version: u32,
    /// The `OrganizationLayout` the members are arranged by.
    pub layout: String,
    /// Each layout slot's element template; empty for an empty slot.
    pub slots: Vec<String>,
    /// The members where they stand, in position order, one for each of
    /// [`Organization::positions`] that's filled.
    pub members: Vec<Thing>,
    pub positions: Vec<Position>,
    /// The position of the member in the lead; -1 for none.
    pub leader: i32,
    /// A fraction of its maximum, the sum of its members'.
    pub morale: f32,
    /// The index of its formation; -1 for none.
    pub formation: i32,
    /// From version 3 on.
    pub desired_formation: Option<i32>,
    /// Version 0 only.
    pub old: Option<u32>,
    /// From version 4 on.
    pub fighting: Option<Fighting>,
    /// From version 5 on: a level its properties set; 1 in every map.
    pub level: Option<f32>,
    /// From version 6 on: a time; 0 in every map.
    pub time: Option<f32>,
    /// From version 8 on: its position, (x, y), as last recorded.
    pub position: Option<[f32; 2]>,
    /// From version 9 on: the thing id of the HQ it's attached to.
    pub hq: Option<u32>,
    /// From version 10 on: the thing id of the transport carrying it.
    pub transport: Option<u32>,
}

/// A place in the organization's layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    /// The member's index; -1 when the position is empty.
    pub index: i32,
    /// Versions 2 to 6.
    pub old: Option<u32>,
    /// From version 7 on: the template of the member that belongs here.
    pub template: Option<String>,
    /// Where it stands relative to the organization.
    pub offset: [f32; 2],
    /// From version 1 on; 0 in every map.
    pub unknown1: Option<u32>,
}

/// What an organization keeps for fighting, from version 4 on.
#[derive(Debug, Clone, PartialEq)]
pub struct Fighting {
    /// Eases over time towards [`Organization::level`].
    pub unknown2: f32,
    pub unknown3: u8,
    pub unknown4: u8,
    pub damage: Damage,
}

/// The damage an element deals.
#[derive(Debug, Clone, PartialEq)]
pub struct Damage {
    pub unknown1: [u32; 2],
    /// The damage type's id.
    pub name: String,
    pub unknown2: [u32; 2],
    pub tail: DamageTail,
}

/// The end of a [`Damage`], whose layout follows the world version.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DamageTail {
    /// Before world version 15.
    None,
    /// World versions 15 to 17.
    Byte(u8),
    /// World versions 18 and 19.
    Word(u32),
    /// From world version 20 on.
    Words([u32; 2]),
}

/// The Element component: what an organization's member keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementState {
    pub version: u32,
    /// From version 1 on.
    pub updatable: Option<Updatable>,
    /// Up to version 1.
    pub old: Option<u8>,
    /// From version 3 on.
    pub damages: Option<Vec<Damage>>,
}

impl Organization {
    const MAX_VERSION: u32 = 10;

    pub(super) fn parse(cur: &mut Cursor, world: u32) -> Result<Self> {
        let updatable = Updatable::parse(cur)?;
        let v = version(cur, "organization", Self::MAX_VERSION)?;
        expect(cur, "organization layout version", 0)?;
        let layout = cur.string()?;
        let n = cur.u32()?;
        let slots = (0..n).map(|_| cur.string()).collect::<Result<_>>()?;
        let n = cur.u32()?;
        let mut positions = Vec::new();
        let mut members = Vec::new();
        for _ in 0..n {
            let index = cur.i32()?;
            if index >= 0 {
                members.push(Thing::parse_member(cur, world)?);
            }
            positions.push(Position {
                index,
                old: None,
                template: None,
                offset: [0.0; 2],
                unknown1: None,
            });
        }
        if (2..=6).contains(&v) {
            for p in &mut positions {
                p.old = Some(cur.u32()?);
            }
        }
        if v >= 7 {
            for p in &mut positions {
                p.template = Some(cur.string()?);
            }
        }
        let leader = cur.i32()?;
        let morale = cur.f32()?;
        let formation = cur.i32()?;
        let desired_formation = (v >= 3).then(|| cur.i32()).transpose()?;
        let old = (v == 0).then(|| cur.u32()).transpose()?;
        for p in &mut positions {
            p.offset = cur.f32s()?;
        }
        if v >= 1 {
            for p in &mut positions {
                p.unknown1 = Some(cur.u32()?);
            }
        }
        let fighting = (v >= 4)
            .then(|| -> Result<_> {
                Ok(Fighting {
                    unknown2: cur.f32()?,
                    unknown3: cur.u8()?,
                    unknown4: cur.u8()?,
                    damage: Damage::parse(cur, world)?,
                })
            })
            .transpose()?;
        let level = (v >= 5).then(|| cur.f32()).transpose()?;
        let time = (v >= 6).then(|| cur.f32()).transpose()?;
        let position = (v >= 8).then(|| cur.f32s()).transpose()?;
        let hq = (v >= 9).then(|| cur.u32()).transpose()?;
        let transport = (v >= 10).then(|| cur.u32()).transpose()?;
        Ok(Organization {
            updatable,
            version: v,
            layout,
            slots,
            members,
            positions,
            leader,
            morale,
            formation,
            desired_formation,
            old,
            fighting,
            level,
            time,
            position,
            hq,
            transport,
        })
    }

    pub(super) fn write(&self, w: &mut Writer) -> Result<()> {
        let v = self.version;
        let filled = self.positions.iter().filter(|p| p.index >= 0).count();
        if filled != self.members.len() {
            return Err(Error::Unwritable(format!(
                "{} members for {filled} filled positions",
                self.members.len()
            )));
        }
        self.updatable.write(w);
        w.u32(v);
        w.u32(0);
        w.string(&self.layout)?;
        w.u32(self.slots.len() as u32);
        for s in &self.slots {
            w.string(s)?;
        }
        w.u32(self.positions.len() as u32);
        let mut members = self.members.iter();
        for p in &self.positions {
            w.i32(p.index);
            if p.index >= 0 {
                members.next().unwrap().write(w)?;
            }
        }
        for p in &self.positions {
            optional((2..=6).contains(&v), "position's old value", p.old, |o| {
                w.u32(o);
                Ok(())
            })?;
        }
        for p in &self.positions {
            optional(v >= 7, "position's template", p.template.as_deref(), |t| {
                w.string(t)
            })?;
        }
        w.i32(self.leader);
        w.f32(self.morale);
        w.i32(self.formation);
        optional(v >= 3, "desired formation", self.desired_formation, |f| {
            w.i32(f);
            Ok(())
        })?;
        optional(v == 0, "organization's old value", self.old, |o| {
            w.u32(o);
            Ok(())
        })?;
        for p in &self.positions {
            w.f32s(&p.offset);
        }
        for p in &self.positions {
            optional(v >= 1, "position's unknown", p.unknown1, |u| {
                w.u32(u);
                Ok(())
            })?;
        }
        optional(v >= 4, "fighting", self.fighting.as_ref(), |f| {
            w.f32(f.unknown2);
            w.u8(f.unknown3);
            w.u8(f.unknown4);
            f.damage.write(w)
        })?;
        let words = [
            (5, "level", self.level.map(f32::to_bits)),
            (6, "time", self.time.map(f32::to_bits)),
        ];
        for (from, what, value) in words {
            optional(v >= from, what, value, |b| {
                w.u32(b);
                Ok(())
            })?;
        }
        optional(v >= 8, "position", self.position, |p| {
            w.f32s(&p);
            Ok(())
        })?;
        for (from, what, value) in [(9, "hq", self.hq), (10, "transport", self.transport)] {
            optional(v >= from, what, value, |id| {
                w.u32(id);
                Ok(())
            })?;
        }
        Ok(())
    }
}

impl Damage {
    fn parse(cur: &mut Cursor, world: u32) -> Result<Self> {
        Ok(Damage {
            unknown1: [cur.u32()?, cur.u32()?],
            name: cur.string()?,
            unknown2: [cur.u32()?, cur.u32()?],
            tail: match world {
                ..15 => DamageTail::None,
                15..18 => DamageTail::Byte(cur.u8()?),
                18..20 => DamageTail::Word(cur.u32()?),
                _ => DamageTail::Words([cur.u32()?, cur.u32()?]),
            },
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        self.unknown1.iter().for_each(|&v| w.u32(v));
        w.string(&self.name)?;
        self.unknown2.iter().for_each(|&v| w.u32(v));
        match self.tail {
            DamageTail::None => {}
            DamageTail::Byte(b) => w.u8(b),
            DamageTail::Word(v) => w.u32(v),
            DamageTail::Words(vs) => vs.iter().for_each(|&v| w.u32(v)),
        }
        Ok(())
    }
}

impl ElementState {
    pub(super) fn parse(cur: &mut Cursor, world: u32) -> Result<Self> {
        let v = version(cur, "element", 3)?;
        Ok(ElementState {
            version: v,
            updatable: (v >= 1).then(|| Updatable::parse(cur)).transpose()?,
            old: (v <= 1).then(|| cur.u8()).transpose()?,
            damages: (v >= 3)
                .then(|| (0..cur.u32()?).map(|_| Damage::parse(cur, world)).collect())
                .transpose()?,
        })
    }

    pub(super) fn write(&self, w: &mut Writer) -> Result<()> {
        let v = self.version;
        w.u32(v);
        optional(v >= 1, "element's updatable", self.updatable, |u| {
            u.write(w);
            Ok(())
        })?;
        optional(v <= 1, "element's old value", self.old, |o| {
            w.u8(o);
            Ok(())
        })?;
        optional(v >= 3, "element's damages", self.damages.as_deref(), |ds| {
            w.u32(ds.len() as u32);
            ds.iter().try_for_each(|d| d.write(w))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn damage(tail: DamageTail) -> Damage {
        Damage {
            unknown1: [1, 2],
            name: "dmg_bullet".into(),
            unknown2: [3, 4],
            tail,
        }
    }

    fn empty(version: u32) -> Organization {
        let has = |from: u32| version >= from;
        Organization {
            updatable: Updatable { version: 1, id: 7 },
            version,
            layout: "herd_ambient_small".into(),
            slots: vec!["sheep_ewe".into()],
            members: Vec::new(),
            positions: vec![Position {
                index: -1,
                old: (2..=6).contains(&version).then_some(0),
                template: has(7).then(String::new),
                offset: [1.5, -2.0],
                unknown1: has(1).then_some(0),
            }],
            leader: -1,
            morale: 1.0,
            formation: 0,
            desired_formation: has(3).then_some(1),
            old: (version == 0).then_some(0),
            fighting: has(4).then(|| Fighting {
                unknown2: 1.0,
                unknown3: 0,
                unknown4: 1,
                damage: damage(DamageTail::Byte(2)),
            }),
            level: has(5).then_some(1.0),
            time: has(6).then_some(0.0),
            position: has(8).then_some([10.0, 20.0]),
            hq: has(9).then_some(0),
            transport: has(10).then_some(0),
        }
    }

    #[test]
    fn round_trips_each_version() {
        // World version 16 keeps a byte at the end of each damage.
        for version in 0..=Organization::MAX_VERSION {
            let org = empty(version);
            let mut w = Writer::default();
            org.write(&mut w).unwrap();
            let mut cur = Cursor::new(&w.0);
            assert_eq!(Organization::parse(&mut cur, 16).unwrap(), org);
            assert!(cur.at_end(), "version {version}");
        }
        let mut wrong = empty(8);
        wrong.hq = Some(3);
        assert!(wrong.write(&mut Writer::default()).is_err(), "no hq in v8");
    }

    #[test]
    fn round_trips_elements() {
        for (version, world, tail) in [
            (3, 20, DamageTail::Words([5, 6])),
            (3, 18, DamageTail::Word(5)),
            (3, 10, DamageTail::None),
        ] {
            let element = ElementState {
                version,
                updatable: Some(Updatable { version: 1, id: 9 }),
                old: None,
                damages: Some(vec![damage(tail)]),
            };
            let mut w = Writer::default();
            element.write(&mut w).unwrap();
            let mut cur = Cursor::new(&w.0);
            assert_eq!(ElementState::parse(&mut cur, world).unwrap(), element);
            assert!(cur.at_end());
        }
        let old = ElementState {
            version: 0,
            updatable: None,
            old: Some(1),
            damages: None,
        };
        let mut w = Writer::default();
        old.write(&mut w).unwrap();
        assert_eq!(w.0, [0, 0, 0, 0, 1]);
    }
}
