//! Readers for "TGCK" files, as used by Axis & Allies (2004).
//!
//! One container format ([`container`]) holds named, optionally zlib-compressed
//! sections. The game uses it for:
//! - `.rwd` file archives ([`archive`])
//! - `.rmp` maps ([`map`])

use std::fmt;
use std::io;
use std::ops::RangeInclusive;

pub mod archive;
pub mod container;
pub mod map;

pub use archive::{Archive, Entry};
pub use container::{Container, Section};
pub use map::Map;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    BadMagic([u8; 4]),
    UnsupportedVersion(u32),
    MissingSection(&'static str),
    Corrupt(String),
    /// Data that can't be written in the format it asks for.
    Unwritable(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::BadMagic(m) => write!(f, "not a TGCK file (magic {m:02x?})"),
            Error::UnsupportedVersion(v) => write!(f, "unsupported TGCK version {v}"),
            Error::MissingSection(s) => write!(f, "file has no {s:?} section"),
            Error::Corrupt(s) => write!(f, "corrupt file: {s}"),
            Error::Unwritable(s) => write!(f, "can't write: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Bounds-checked little-endian reader over a section payload.
pub(crate) struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Cursor { buf, pos: 0 }
    }
    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        let end =
            end.ok_or_else(|| Error::Corrupt(format!("truncated at byte {} (+{n})", self.pos)))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    pub(crate) fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub(crate) fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
    pub(crate) fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub(crate) fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub(crate) fn f32s<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }
        Ok(out)
    }
    /// `u16` length in code units, then UTF-16LE. Decoded strictly, NULs
    /// and all, so that a string saves back as it was read.
    pub(crate) fn string(&mut self) -> Result<String> {
        let at = self.pos();
        let len = self.u16()? as usize;
        let units: Vec<u16> = self
            .bytes(len * 2)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        String::from_utf16(&units)
            .map_err(|_| Error::Corrupt(format!("string at byte {at} isn't valid UTF-16")))
    }
    /// A `u32` count, then that many items; what's reserved up front is
    /// capped by the bytes left so a corrupt count can't reserve too much.
    pub(crate) fn list<T>(
        &mut self,
        mut item: impl FnMut(&mut Cursor<'a>) -> Result<T>,
    ) -> Result<Vec<T>> {
        let n = self.u32()? as usize;
        let mut items = Vec::with_capacity(n.min(self.remaining() / size_of::<T>().max(1)));
        for _ in 0..n {
            items.push(item(self)?);
        }
        Ok(items)
    }
    /// A `u32` count, then that many slots, each a presence flag and, if
    /// set, an item read knowing its slot's index.
    pub(crate) fn slots<T>(
        &mut self,
        what: &str,
        mut item: impl FnMut(&mut Cursor<'a>, u32) -> Result<T>,
    ) -> Result<Vec<Option<T>>> {
        let n = self.u32()?;
        (0..n)
            .map(|index| self.bool(what)?.then(|| item(self, index)).transpose())
            .collect()
    }
    pub(crate) fn u32s(&mut self) -> Result<Vec<u32>> {
        self.list(Cursor::u32)
    }
    pub(crate) fn strings(&mut self) -> Result<Vec<String>> {
        self.list(Cursor::string)
    }
    /// A list of `(string, value)` pairs.
    pub(crate) fn pairs<T>(
        &mut self,
        mut value: impl FnMut(&mut Cursor<'a>) -> Result<T>,
    ) -> Result<Vec<(String, T)>> {
        self.list(|cur| Ok((cur.string()?, value(cur)?)))
    }
    /// A byte that must be 0 or 1.
    pub(crate) fn bool(&mut self, what: &str) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(Error::Corrupt(format!("{what} is {b}"))),
        }
    }
    /// A `u32` version, checked with [`check_version`].
    pub(crate) fn version(&mut self, what: &str, versions: RangeInclusive<u32>) -> Result<u32> {
        check_version(what, self.u32()?, versions)
    }
    /// A `u32` that has only one valid value, so that writing that value
    /// back reproduces the file.
    pub(crate) fn expect(&mut self, what: &str, want: u32) -> Result<()> {
        match self.u32()? {
            v if v == want => Ok(()),
            v => Err(Error::Corrupt(format!("{what} is {v}, expected {want}"))),
        }
    }
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }
    pub(crate) fn seek(&mut self, pos: usize) {
        self.pos = pos.min(self.buf.len());
    }
    pub(crate) fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
    pub(crate) fn at_end(&self) -> bool {
        self.pos == self.buf.len()
    }
    /// What was read since `from`.
    pub(crate) fn since(&self, from: usize) -> &'a [u8] {
        &self.buf[from..self.pos]
    }
    pub(crate) fn rest(&mut self) -> &'a [u8] {
        let s = &self.buf[self.pos..];
        self.pos = self.buf.len();
        s
    }
    pub(crate) fn finish(&self, what: &str) -> Result<()> {
        match self.buf.len() - self.pos {
            0 => Ok(()),
            n => Err(Error::Corrupt(format!("{what} has {n} trailing bytes"))),
        }
    }
}

/// The inverse of [`Cursor`].
#[derive(Default)]
pub(crate) struct Writer(pub(crate) Vec<u8>);

impl Writer {
    pub(crate) fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }
    pub(crate) fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub(crate) fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }
    pub(crate) fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }
    pub(crate) fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    pub(crate) fn i32(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }
    pub(crate) fn f32(&mut self, v: f32) {
        self.bytes(&v.to_le_bytes());
    }
    pub(crate) fn f32s(&mut self, vs: &[f32]) {
        vs.iter().for_each(|&v| self.f32(v));
    }
    pub(crate) fn string(&mut self, s: &str) -> Result<()> {
        let units: Vec<u16> = s.encode_utf16().collect();
        let len = u16::try_from(units.len())
            .map_err(|_| Error::Unwritable(format!("{} UTF-16 units in a string", units.len())))?;
        self.u16(len);
        units.iter().for_each(|&u| self.u16(u));
        Ok(())
    }
    /// A list's `u32` count; `what` names the list if it's too long.
    pub(crate) fn count(&mut self, what: &str, n: usize) -> Result<()> {
        let n = u32::try_from(n).map_err(|_| Error::Unwritable(format!("{n} {what}")))?;
        self.u32(n);
        Ok(())
    }
    /// The inverse of [`Cursor::list`].
    pub(crate) fn list<T>(
        &mut self,
        what: &str,
        items: &[T],
        mut item: impl FnMut(&mut Writer, &T) -> Result<()>,
    ) -> Result<()> {
        self.count(what, items.len())?;
        items.iter().try_for_each(|i| item(self, i))
    }
    /// The inverse of [`Cursor::slots`].
    pub(crate) fn slots<T>(
        &mut self,
        what: &str,
        slots: &[Option<T>],
        mut item: impl FnMut(&mut Writer, &T, u32) -> Result<()>,
    ) -> Result<()> {
        self.count(what, slots.len())?;
        for (index, slot) in (0..).zip(slots) {
            self.u8(slot.is_some().into());
            if let Some(slot) = slot {
                item(self, slot, index)?;
            }
        }
        Ok(())
    }
    /// The inverse of [`Cursor::u32s`].
    pub(crate) fn u32s(&mut self, what: &str, vs: &[u32]) -> Result<()> {
        self.count(what, vs.len())?;
        vs.iter().for_each(|&v| self.u32(v));
        Ok(())
    }
    /// The inverse of `Cursor::list(Cursor::f32)`.
    pub(crate) fn f32_list(&mut self, what: &str, vs: &[f32]) -> Result<()> {
        self.count(what, vs.len())?;
        self.f32s(vs);
        Ok(())
    }
    /// The inverse of [`Cursor::strings`].
    pub(crate) fn strings(&mut self, what: &str, vs: &[String]) -> Result<()> {
        self.list(what, vs, |w, s| w.string(s))
    }
    /// The inverse of [`Cursor::pairs`].
    pub(crate) fn pairs<T: Copy>(
        &mut self,
        what: &str,
        pairs: &[(String, T)],
        mut value: impl FnMut(&mut Writer, T),
    ) -> Result<()> {
        self.list(what, pairs, |w, (s, v)| {
            w.string(s)?;
            value(w, *v);
            Ok(())
        })
    }
    /// Writes `value` with `write` if the version has it, checking that it's
    /// set exactly when it does.
    pub(crate) fn optional<T>(
        &mut self,
        has: bool,
        what: &str,
        value: Option<T>,
        write: impl FnOnce(&mut Writer, T) -> Result<()>,
    ) -> Result<()> {
        match (has, value) {
            (true, Some(v)) => write(self, v),
            (false, None) => Ok(()),
            _ => Err(Error::Unwritable(format!(
                "{what} must be set exactly when its version has it"
            ))),
        }
    }
    /// [`Writer::optional`] for a value written by a call that can't fail.
    pub(crate) fn optional_plain<T>(
        &mut self,
        has: bool,
        what: &str,
        value: Option<T>,
        write: impl FnOnce(&mut Writer, T),
    ) -> Result<()> {
        self.optional(has, what, value, |w, v| {
            write(w, v);
            Ok(())
        })
    }
}

/// `version`, if it's one of `versions`.
pub(crate) fn check_version(
    what: &str,
    version: u32,
    versions: RangeInclusive<u32>,
) -> Result<u32> {
    if versions.contains(&version) {
        Ok(version)
    } else {
        Err(Error::Corrupt(format!("{what} version {version}")))
    }
}

/// Decodes UTF-16LE up to the first NUL, replacing invalid units, as for a
/// fixed-size padded name like a section's.
pub(crate) fn utf16_lossy(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
pub(crate) mod tests {
    pub fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// A string as [`crate::Cursor::string`] reads it.
    pub fn string(s: &str) -> Vec<u8> {
        let mut b = (s.encode_utf16().count() as u16).to_le_bytes().to_vec();
        b.extend(utf16(s));
        b
    }

    /// Builds a container from `(name, payload, compress)` triples.
    pub fn build_container(sections: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = crate::container::MAGIC.to_vec();
        out.extend(crate::container::VERSION.to_le_bytes());
        out.extend((sections.len() as u32).to_le_bytes());
        let mut table = Vec::new();
        for (name, payload, compress) in sections {
            let stored = if *compress {
                miniz_oxide::deflate::compress_to_vec_zlib(payload, 6)
            } else {
                payload.to_vec()
            };
            let mut rec = utf16(name);
            rec.resize(0x40, 0);
            rec.extend((out.len() as u64).to_le_bytes());
            rec.extend((stored.len() as u64).to_le_bytes());
            rec.extend((*compress as u64).to_le_bytes());
            rec.extend((payload.len() as u64).to_le_bytes());
            table.extend(rec);
            out.extend(stored);
        }
        out.extend(table);
        out
    }

    #[test]
    fn reads_strings_as_they_will_be_written() {
        let s = "a\0b\u{e9}";
        let bytes = string(s);
        assert_eq!(crate::Cursor::new(&bytes).string().unwrap(), s);
        let mut w = crate::Writer::default();
        w.string(s).unwrap();
        assert_eq!(w.0, bytes);

        // An unpaired surrogate can't be held in a `String`.
        let bad = [1, 0, 0x00, 0xd8];
        assert!(crate::Cursor::new(&bad).string().is_err());
    }
}
