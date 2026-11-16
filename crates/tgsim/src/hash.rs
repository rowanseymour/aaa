//! A number that two copies of a match share only while they've played out
//! the same way, for telling when they haven't.

use std::collections::{BTreeMap, VecDeque};

use tgmap::{Id, Tile};

use crate::World;

/// FNV-1a, which unlike std's hashers is fixed for good.
pub(crate) struct Hasher(u64);

impl Hasher {
    pub(crate) fn new() -> Self {
        Hasher(0xcbf2_9ce4_8422_2325)
    }

    pub(crate) fn finish(&self) -> u64 {
        self.0
    }

    /// Takes in `bits` sixty-four at a time, for grids of them too big to
    /// take in one by one.
    pub(crate) fn bits(&mut self, bits: &[bool]) {
        bits.len().digest(self);
        for chunk in bits.chunks(64) {
            let word = chunk.iter().rev().fold(0u64, |w, &b| w << 1 | u64::from(b));
            self.0 = (self.0 ^ word).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// What goes into a match's hash: everything orders and ticks can change.
pub(crate) trait Digest {
    fn digest(&self, h: &mut Hasher);
}

/// Has a struct go into the hash field by field. Every field is named,
/// those after `skip` as ones a match never changes or that follow from
/// the rest, so that a new field has to be put with one or the other.
macro_rules! digest {
    ($ty:ty { $($field:ident),* $(,)? } $(skip { $($skip:ident),* $(,)? })?) => {
        impl $crate::hash::Digest for $ty {
            fn digest(&self, h: &mut $crate::hash::Hasher) {
                let Self { $($field,)* $($($skip: _,)*)? } = self;
                $($crate::hash::Digest::digest($field, h);)*
            }
        }
    };
}
pub(crate) use digest;

macro_rules! digest_numbers {
    ($($ty:ty),*) => {$(
        impl Digest for $ty {
            fn digest(&self, h: &mut Hasher) {
                h.write(&self.to_le_bytes());
            }
        }
    )*};
}
digest_numbers!(u8, u16, u32, u64, i32);

impl Digest for usize {
    fn digest(&self, h: &mut Hasher) {
        (*self as u64).digest(h);
    }
}

impl Digest for bool {
    fn digest(&self, h: &mut Hasher) {
        u8::from(*self).digest(h);
    }
}

impl Digest for f32 {
    /// Every NaN as one: which a sum comes to differs between processors.
    fn digest(&self, h: &mut Hasher) {
        match self.is_nan() {
            true => f32::NAN.to_bits().digest(h),
            false => self.to_bits().digest(h),
        }
    }
}

impl Digest for str {
    fn digest(&self, h: &mut Hasher) {
        self.len().digest(h);
        h.write(self.as_bytes());
    }
}

impl Digest for String {
    fn digest(&self, h: &mut Hasher) {
        self.as_str().digest(h);
    }
}

impl Digest for Id {
    /// Whatever its case, as it's compared.
    fn digest(&self, h: &mut Hasher) {
        self.as_str().len().digest(h);
        for b in self.as_str().bytes() {
            h.write(&[b.to_ascii_lowercase()]);
        }
    }
}

impl Digest for Tile {
    fn digest(&self, h: &mut Hasher) {
        (self.x, self.y).digest(h);
    }
}

impl<T: Digest + ?Sized> Digest for &T {
    fn digest(&self, h: &mut Hasher) {
        (**self).digest(h);
    }
}

impl<T: Digest + ?Sized> Digest for Box<T> {
    fn digest(&self, h: &mut Hasher) {
        (**self).digest(h);
    }
}

impl<T: Digest> Digest for Option<T> {
    fn digest(&self, h: &mut Hasher) {
        self.is_some().digest(h);
        if let Some(x) = self {
            x.digest(h);
        }
    }
}

impl<T: Digest> Digest for [T] {
    fn digest(&self, h: &mut Hasher) {
        self.len().digest(h);
        self.iter().for_each(|x| x.digest(h));
    }
}

impl<T: Digest> Digest for Vec<T> {
    fn digest(&self, h: &mut Hasher) {
        self.as_slice().digest(h);
    }
}

impl<T: Digest> Digest for VecDeque<T> {
    fn digest(&self, h: &mut Hasher) {
        self.len().digest(h);
        self.iter().for_each(|x| x.digest(h));
    }
}

impl<T: Digest, const N: usize> Digest for [T; N] {
    fn digest(&self, h: &mut Hasher) {
        self.iter().for_each(|x| x.digest(h));
    }
}

impl<K: Digest, V: Digest> Digest for BTreeMap<K, V> {
    fn digest(&self, h: &mut Hasher) {
        self.len().digest(h);
        self.iter().for_each(|entry| entry.digest(h));
    }
}

impl<A: Digest, B: Digest> Digest for (A, B) {
    fn digest(&self, h: &mut Hasher) {
        self.0.digest(h);
        self.1.digest(h);
    }
}

impl World {
    /// A number the same in every copy of the match that has been given
    /// the same orders on the same ticks, and all but surely different in
    /// one that has played out any other way.
    pub fn hash(&self) -> u64 {
        let mut h = Hasher::new();
        self.digest(&mut h);
        h.finish()
    }
}
