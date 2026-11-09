//! The Organization and Element components: a herd or regiment holding its
//! members' whole records, and what each member keeps of the damage it
//! deals. Laid out as `organization` and `element` in [`rmp.ksy`].
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use super::actor::{Updatable, version};
use super::header::{optional, optional_plain};
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
    /// From version 1 on, for a position whose member was lost: the health
    /// its replacement has built up, the member returning at full health.
    /// 0 for a filled position.
    pub replenished: Option<f32>,
}

/// What an organization keeps for fighting, from version 4 on.
#[derive(Debug, Clone, PartialEq)]
pub struct Fighting {
    /// Its attack efficiency, which eases over time towards
    /// [`Organization::level`].
    pub efficiency: f32,
    /// Whether its formation is an aggressive one.
    pub formation_aggressive: u8,
    /// Whether it's aggressive now.
    pub aggressive: u8,
    /// The last hit any of its members took.
    pub damage: Damage,
}

/// A hit an element or organization took.
#[derive(Debug, Clone, PartialEq)]
pub struct Damage {
    /// The attacker's thing id.
    pub attacker: u32,
    /// The thing id of the one credited with the attack when that's someone
    /// else, such as the commander for its own abilities; 0 otherwise.
    pub credited: u32,
    /// The damage type's id.
    pub name: String,
    pub amount: f32,
    /// The game time it landed.
    pub time: f32,
    pub tail: DamageTail,
}

/// The end of a [`Damage`], whose layout follows the world version.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DamageTail {
    /// Before world version 15.
    None,
    /// World versions 15 to 17: a byte the game ignores.
    Byte(u8),
    /// World versions 18 and 19: `use_kind`, as `damage` in [`rmp.ksy`]
    /// describes.
    ///
    /// [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy
    Word(u32),
    /// From world version 20 on: `use_kind`, then `cause`.
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
    /// From version 3 on: the hits it took lately, kept only while it's the
    /// member whose fate its regiment watches.
    pub damages: Option<Vec<Damage>>,
}

impl Organization {
    const MAX_VERSION: u32 = 10;
    /// Members are actors that may be organizations themselves; real maps go
    /// one level down, and each level costs far more stack than input.
    const MAX_DEPTH: u32 = 8;

    #[cfg(test)]
    pub(super) fn parse(cur: &mut Cursor, world: u32) -> Result<Self> {
        Organization::parse_nested(cur, world, 0)
    }

    /// Reads an organization `depth` organizations down.
    pub(super) fn parse_nested(cur: &mut Cursor, world: u32, depth: u32) -> Result<Self> {
        if depth >= Self::MAX_DEPTH {
            return Err(Error::Corrupt(format!(
                "organizations nested more than {} deep",
                Self::MAX_DEPTH
            )));
        }
        let updatable = Updatable::parse(cur)?;
        let v = version(cur, "organization", Self::MAX_VERSION)?;
        expect(cur, "organization layout version", 0)?;
        let layout = cur.string()?;
        let slots = cur.strings()?;
        let n = cur.u32()?;
        let mut positions = Vec::new();
        let mut members = Vec::new();
        for _ in 0..n {
            let index = cur.i32()?;
            if index >= 0 {
                members.push(Thing::parse_member(cur, world, depth + 1)?);
            }
            positions.push(Position {
                index,
                old: None,
                template: None,
                offset: [0.0; 2],
                replenished: None,
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
                p.replenished = Some(cur.f32()?);
            }
        }
        let fighting = (v >= 4)
            .then(|| -> Result<_> {
                Ok(Fighting {
                    efficiency: cur.f32()?,
                    formation_aggressive: cur.u8()?,
                    aggressive: cur.u8()?,
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
        w.strings(&self.slots)?;
        w.u32(self.positions.len() as u32);
        let mut members = self.members.iter();
        for p in &self.positions {
            w.i32(p.index);
            if p.index >= 0 {
                members.next().unwrap().write(w)?;
            }
        }
        for p in &self.positions {
            optional_plain((2..=6).contains(&v), "position's old value", p.old, |o| {
                w.u32(o)
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
        optional_plain(v >= 3, "desired formation", self.desired_formation, |f| {
            w.i32(f)
        })?;
        optional_plain(v == 0, "organization's old value", self.old, |o| w.u32(o))?;
        for p in &self.positions {
            w.f32s(&p.offset);
        }
        for p in &self.positions {
            optional_plain(v >= 1, "position's replenishment", p.replenished, |r| {
                w.f32(r)
            })?;
        }
        optional(v >= 4, "fighting", self.fighting.as_ref(), |f| {
            w.f32(f.efficiency);
            w.u8(f.formation_aggressive);
            w.u8(f.aggressive);
            f.damage.write(w)
        })?;
        let words = [
            (5, "level", self.level.map(f32::to_bits)),
            (6, "time", self.time.map(f32::to_bits)),
        ];
        for (from, what, value) in words {
            optional_plain(v >= from, what, value, |b| w.u32(b))?;
        }
        optional_plain(v >= 8, "position", self.position, |p| w.f32s(&p))?;
        for (from, what, value) in [(9, "hq", self.hq), (10, "transport", self.transport)] {
            optional_plain(v >= from, what, value, |id| w.u32(id))?;
        }
        Ok(())
    }
}

impl Damage {
    fn parse(cur: &mut Cursor, world: u32) -> Result<Self> {
        Ok(Damage {
            attacker: cur.u32()?,
            credited: cur.u32()?,
            name: cur.string()?,
            amount: cur.f32()?,
            time: cur.f32()?,
            tail: match world {
                ..15 => DamageTail::None,
                15..18 => DamageTail::Byte(cur.u8()?),
                18..20 => DamageTail::Word(cur.u32()?),
                _ => DamageTail::Words([cur.u32()?, cur.u32()?]),
            },
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(self.attacker);
        w.u32(self.credited);
        w.string(&self.name)?;
        w.f32(self.amount);
        w.f32(self.time);
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
        optional_plain(v >= 1, "element's updatable", self.updatable, |u| {
            u.write(w)
        })?;
        optional_plain(v <= 1, "element's old value", self.old, |o| w.u8(o))?;
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
            attacker: 1,
            credited: 2,
            name: "dmg_bullet".into(),
            amount: 3.0,
            time: 4.0,
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
                replenished: has(1).then_some(0.0),
            }],
            leader: -1,
            morale: 1.0,
            formation: 0,
            desired_formation: has(3).then_some(1),
            old: (version == 0).then_some(0),
            fighting: has(4).then(|| Fighting {
                efficiency: 1.0,
                formation_aggressive: 0,
                aggressive: 1,
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
