//! The simulation's bookkeeping: IDMGR and UPDATABLES, laid out as [`rmp.ksy`]
//! specifies.
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use crate::{Cursor, Result, Writer, check_version};

/// The IDMGR section, the `id_manager` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct IdManager {
    pub next_slot: u32,
    pub generations: Vec<u16>,
}

impl IdManager {
    const CAPACITY: u32 = 0x10000;

    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let cur = &mut cur;
        cur.expect("IDMGR version", 0)?;
        let next_slot = cur.u32()?;
        let highest_slot = cur.u32()?;
        cur.expect("IDMGR capacity", Self::CAPACITY)?;
        let generations = (0..highest_slot)
            .map(|_| cur.u16())
            .collect::<Result<_>>()?;
        cur.finish("IDMGR")?;
        Ok(IdManager {
            next_slot,
            generations,
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        w.u32(0);
        w.u32(self.next_slot);
        w.count("generations", self.generations.len())?;
        w.u32(Self::CAPACITY);
        self.generations.iter().for_each(|&g| w.u16(g));
        Ok(w.0)
    }
}

/// The UPDATABLES section, the `updatables` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Updatables {
    pub count: u32,
    pub delayed: [UpdateQueue; 2],
    pub periodic: Option<[UpdateQueue; 3]>,
}

/// An `update_queue` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateQueue {
    pub current_slot: u32,
    /// Each `update_slot`'s updates.
    pub slots: Vec<Vec<Update>>,
}

/// An `update` of `rmp.ksy`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Update {
    pub updatable_id: i32,
    pub argument: u32,
}

impl Updatables {
    /// The first world version whose UPDATABLES the game reads.
    const MIN_WORLD_VERSION: u32 = 11;
    /// The first world version with the periodic queues.
    const PERIODIC_WORLD_VERSION: u32 = 22;

    pub fn parse(data: &[u8], world_version: u32) -> Result<Self> {
        check_version(
            "UPDATABLES world",
            world_version,
            Self::MIN_WORLD_VERSION..=u32::MAX,
        )?;
        let mut cur = Cursor::new(data);
        let cur = &mut cur;
        let updatables = Updatables {
            count: cur.u32()?,
            delayed: [UpdateQueue::parse(cur)?, UpdateQueue::parse(cur)?],
            periodic: (world_version >= Self::PERIODIC_WORLD_VERSION)
                .then(|| -> Result<_> {
                    Ok([
                        UpdateQueue::parse(cur)?,
                        UpdateQueue::parse(cur)?,
                        UpdateQueue::parse(cur)?,
                    ])
                })
                .transpose()?,
        };
        cur.finish("UPDATABLES")?;
        Ok(updatables)
    }

    pub fn to_bytes(&self, world_version: u32) -> Result<Vec<u8>> {
        check_version(
            "UPDATABLES world",
            world_version,
            Self::MIN_WORLD_VERSION..=u32::MAX,
        )?;
        let mut w = Writer::default();
        w.u32(self.count);
        self.delayed.iter().try_for_each(|q| q.write(&mut w))?;
        w.optional(
            world_version >= Self::PERIODIC_WORLD_VERSION,
            "periodic",
            self.periodic.as_ref(),
            |w, queues| queues.iter().try_for_each(|q| q.write(w)),
        )?;
        Ok(w.0)
    }
}

impl UpdateQueue {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        cur.expect("update queue version", 0)?;
        let current_slot = cur.u32()?;
        let slots = cur.list(|cur| {
            cur.list(|cur| {
                Ok(Update {
                    updatable_id: cur.i32()?,
                    argument: cur.u32()?,
                })
            })
        })?;
        Ok(UpdateQueue {
            current_slot,
            slots,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(0);
        w.u32(self.current_slot);
        w.list("slots", &self.slots, |w, slot| {
            w.list("updates", slot, |w, update| {
                w.i32(update.updatable_id);
                w.u32(update.argument);
                Ok(())
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_ids() {
        let ids = IdManager {
            next_slot: 4,
            generations: vec![0, 1, 2],
        };
        let bytes = ids.to_bytes().unwrap();
        assert_eq!(bytes.len(), 16 + 6);
        assert_eq!(&bytes[8..16], &[3, 0, 0, 0, 0, 0, 1, 0]);
        assert_eq!(IdManager::parse(&bytes).unwrap(), ids);
        let mut wrong = bytes.clone();
        wrong[14] = 2;
        assert!(IdManager::parse(&wrong).is_err(), "capacity");
        assert!(IdManager::parse(&bytes[..21]).is_err(), "truncated");
    }

    fn queue(slots: usize) -> UpdateQueue {
        let mut slots = vec![Vec::new(); slots];
        slots[0].push(Update {
            updatable_id: 50,
            argument: 0,
        });
        UpdateQueue {
            current_slot: 0,
            slots,
        }
    }

    #[test]
    fn round_trips_updatables() {
        let periodic = Some([queue(1), queue(16), queue(80)]);
        for (world_version, periodic) in [(21, None), (28, periodic)] {
            let updatables = Updatables {
                count: 97,
                delayed: [queue(97), queue(80)],
                periodic,
            };
            let bytes = updatables.to_bytes(world_version).unwrap();
            assert_eq!(
                Updatables::parse(&bytes, world_version).unwrap(),
                updatables
            );
            assert!(
                updatables
                    .to_bytes(if world_version < Updatables::PERIODIC_WORLD_VERSION {
                        Updatables::PERIODIC_WORLD_VERSION
                    } else {
                        Updatables::PERIODIC_WORLD_VERSION - 1
                    })
                    .is_err()
            );
        }
    }

    #[test]
    fn lays_out_update_queues() {
        let mut w = Writer::default();
        queue(2).write(&mut w).unwrap();
        let want = [0u32, 0, 2, 1, 50, 0, 0];
        assert_eq!(w.0, want.map(u32::to_le_bytes).concat());
    }
}
