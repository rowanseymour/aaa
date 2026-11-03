//! The simulation's bookkeeping: IDMGR and UPDATABLES, laid out as [`rmp.ksy`]
//! specifies.
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use super::expect;
use super::header::{check_version, optional};
use crate::{Cursor, Result, Writer};

/// The game's ids for objects (GTIDs): a 16-bit slot, with the slot's
/// generation in the top 16 bits so a reused slot gets a fresh id.
#[derive(Debug, Clone, PartialEq)]
pub struct IdManager {
    /// Where the search for a free slot starts.
    pub next_slot: u32,
    /// Each slot's generation, up to the highest slot ever used; it goes up
    /// when the slot's object is removed.
    pub generations: Vec<u16>,
}

impl IdManager {
    const CAPACITY: u32 = 0x10000;

    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let cur = &mut cur;
        expect(cur, "IDMGR version", 0)?;
        let next_slot = cur.u32()?;
        let highest_slot = cur.u32()?;
        expect(cur, "IDMGR capacity", Self::CAPACITY)?;
        let generations = (0..highest_slot)
            .map(|_| cur.u16())
            .collect::<Result<_>>()?;
        cur.finish("IDMGR")?;
        Ok(IdManager {
            next_slot,
            generations,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(0);
        w.u32(self.next_slot);
        w.u32(self.generations.len() as u32);
        w.u32(Self::CAPACITY);
        self.generations.iter().for_each(|&g| w.u16(g));
        w.0
    }
}

/// The queues that schedule objects' updates.
#[derive(Debug, Clone, PartialEq)]
pub struct Updatables {
    /// How many updatables the world has; ids run up to it.
    pub count: u32,
    /// Ring buffers of ticks, each slot holding the updates due then.
    pub delayed: [UpdateQueue; 2],
    /// One slot per tick of a period of 1, 16 and 80 ticks; from world
    /// version 22 on.
    pub periodic: Option<[UpdateQueue; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UpdateQueue {
    pub current_slot: u32,
    pub slots: Vec<Vec<Update>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Update {
    pub updatable_id: i32,
    /// Passed to the updatable when the update runs.
    pub argument: u32,
}

impl Updatables {
    /// The first world version whose UPDATABLES the game reads.
    const MIN_WORLD_VERSION: u32 = 11;

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
            periodic: (world_version >= 22)
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
        self.delayed.iter().for_each(|q| q.write(&mut w));
        optional(
            world_version >= 22,
            "periodic",
            self.periodic.as_ref(),
            |queues| {
                queues.iter().for_each(|q| q.write(&mut w));
                Ok(())
            },
        )?;
        Ok(w.0)
    }
}

impl UpdateQueue {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        expect(cur, "update queue version", 0)?;
        let current_slot = cur.u32()?;
        let n = cur.u32()?;
        let slots = (0..n)
            .map(|_| {
                let n = cur.u32()?;
                (0..n)
                    .map(|_| {
                        Ok(Update {
                            updatable_id: cur.i32()?,
                            argument: cur.u32()?,
                        })
                    })
                    .collect()
            })
            .collect::<Result<_>>()?;
        Ok(UpdateQueue {
            current_slot,
            slots,
        })
    }

    fn write(&self, w: &mut Writer) {
        w.u32(0);
        w.u32(self.current_slot);
        w.u32(self.slots.len() as u32);
        for slot in &self.slots {
            w.u32(slot.len() as u32);
            for update in slot {
                w.i32(update.updatable_id);
                w.u32(update.argument);
            }
        }
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
        let bytes = ids.to_bytes();
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
                    .to_bytes(if world_version < 22 { 22 } else { 21 })
                    .is_err()
            );
        }
    }

    #[test]
    fn lays_out_update_queues() {
        let mut w = Writer::default();
        queue(2).write(&mut w);
        let want = [0u32, 0, 2, 1, 50, 0, 0];
        assert_eq!(w.0, want.map(u32::to_le_bytes).concat());
    }
}
