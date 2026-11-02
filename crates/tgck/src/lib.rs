//! Readers for "TGCK" files, as used by Axis & Allies (2004).
//!
//! One container format ([`container`]) holds named, optionally zlib-compressed
//! sections. The game uses it for:
//! - `.rwd` file archives ([`archive`])
//! - `.rmp` maps ([`map`])

use std::fmt;
use std::io;

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
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::BadMagic(m) => write!(f, "not a TGCK file (magic {m:02x?})"),
            Error::UnsupportedVersion(v) => write!(f, "unsupported TGCK version {v}"),
            Error::MissingSection(s) => write!(f, "file has no {s:?} section"),
            Error::Corrupt(s) => write!(f, "corrupt file: {s}"),
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
    pub(crate) fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub(crate) fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
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
    /// `u16` length in code units, then UTF-16LE.
    pub(crate) fn string(&mut self) -> Result<String> {
        let len = self.u16()? as usize;
        Ok(utf16_lossy(self.bytes(len * 2)?))
    }
    pub(crate) fn pos(&self) -> usize {
        self.pos
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

    /// Builds a container the way the game lays them out: sections from 0x0C,
    /// then the section table. `(name, payload, compress)`.
    pub fn build_container(sections: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = b"TGCK".to_vec();
        out.extend(2u32.to_le_bytes());
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
            rec.extend([0; 8]);
            rec.extend((payload.len() as u64).to_le_bytes());
            table.extend(rec);
            out.extend(stored);
        }
        out.extend(table);
        out
    }
}
