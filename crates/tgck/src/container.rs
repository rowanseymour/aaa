//! The TGCK section container shared by `.rwd` archives and `.rmp` maps, laid
//! out as [`tgck.ksy`] specifies.
//!
//! [`Section`] keeps the flags' unused bits and the unknown word, though the
//! game ignores them, so that files write back unchanged.
//!
//! [`tgck.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/tgck.ksy

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::{Cursor, Error, Result, Writer, utf16_lossy};

pub const MAGIC: &[u8; 4] = b"TGCK";
const SECTION_RECORD_LEN: u64 = 0x60;
const SECTION_NAME_LEN: usize = 0x40;
/// The zlib level the game compresses sections with.
const ZLIB_LEVEL: u8 = 6;
/// Refuse to inflate sections claiming more than this; real ones are a few MB.
const MAX_UNPACKED: u64 = 256 << 20;

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub offset: u64,
    pub stored_size: u64,
    pub unpacked_size: u64,
    pub flags: u32,
    pub unknown: u32,
}

impl Section {
    pub fn is_compressed(&self) -> bool {
        self.flags & 1 != 0
    }
}

pub struct Container<R> {
    pub(crate) reader: R,
    pub version: u32,
    pub sections: Vec<Section>,
}

impl Container<BufReader<File>> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::new(BufReader::new(File::open(path)?))
    }
}

impl<R: Read + Seek> Container<R> {
    pub fn new(mut reader: R) -> Result<Self> {
        let len = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;

        let mut head = [0u8; 12];
        reader.read_exact(&mut head)?;
        let mut cur = Cursor::new(&head);
        let magic: [u8; 4] = cur.bytes(4)?.try_into().unwrap();
        if &magic != MAGIC {
            return Err(Error::BadMagic(magic));
        }
        let version = cur.u32()?;
        if version != 2 {
            return Err(Error::UnsupportedVersion(version));
        }
        let section_count = cur.u32()? as u64;

        let table_len = section_count * SECTION_RECORD_LEN;
        if table_len > len {
            return Err(Error::Corrupt(format!(
                "{section_count} sections don't fit in {len} bytes"
            )));
        }
        reader.seek(SeekFrom::Start(len - table_len))?;
        let mut sections = Vec::with_capacity(section_count as usize);
        for _ in 0..section_count {
            let mut rec = [0u8; SECTION_RECORD_LEN as usize];
            reader.read_exact(&mut rec)?;
            let mut cur = Cursor::new(&rec);
            let section = Section {
                name: utf16_lossy(cur.bytes(SECTION_NAME_LEN)?),
                offset: cur.u64()?,
                stored_size: cur.u64()?,
                flags: cur.u32()?,
                unknown: cur.u32()?,
                unpacked_size: cur.u64()?,
            };
            if section
                .offset
                .checked_add(section.stored_size)
                .is_none_or(|end| end > len)
            {
                return Err(Error::Corrupt(format!(
                    "section {:?} out of bounds",
                    section.name
                )));
            }
            sections.push(section);
        }
        Ok(Container {
            reader,
            version,
            sections,
        })
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// Like [`Container::section`], but a missing section is an error.
    pub fn require(&self, name: &'static str) -> Result<&Section> {
        self.section(name).ok_or(Error::MissingSection(name))
    }

    /// Reads a section's payload, inflating it if compressed.
    pub fn read_section(&mut self, section: &Section) -> Result<Vec<u8>> {
        self.reader.seek(SeekFrom::Start(section.offset))?;
        let mut stored = vec![0u8; section.stored_size as usize];
        self.reader.read_exact(&mut stored)?;
        if !section.is_compressed() {
            return Ok(stored);
        }
        if section.unpacked_size > MAX_UNPACKED {
            return Err(Error::Corrupt(format!(
                "section {:?} claims {} bytes",
                section.name, section.unpacked_size
            )));
        }
        let data = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(
            &stored,
            section.unpacked_size as usize,
        )
        .map_err(|e| Error::Corrupt(format!("section {:?}: zlib: {e:?}", section.name)))?;
        if data.len() as u64 != section.unpacked_size {
            return Err(Error::Corrupt(format!(
                "section {:?} inflated to {} bytes, expected {}",
                section.name,
                data.len(),
                section.unpacked_size
            )));
        }
        Ok(data)
    }

    pub fn read_named(&mut self, name: &'static str) -> Result<Vec<u8>> {
        let section = self.require(name)?.clone();
        self.read_section(&section)
    }
}

/// Writes a container of `(section, payload)` pairs, laid out as the game
/// does. Each section gives its name and flags, which say whether to
/// compress; its offset and sizes are recomputed from the payload.
pub fn write(out: &mut impl Write, sections: &[(&Section, &[u8])]) -> Result<()> {
    let mut w = Writer::default();
    w.bytes(MAGIC);
    w.u32(2);
    w.u32(sections.len() as u32);
    let mut table = Writer::default();
    for (section, payload) in sections {
        let stored = if section.is_compressed() {
            miniz_oxide::deflate::compress_to_vec_zlib(payload, ZLIB_LEVEL)
        } else {
            payload.to_vec()
        };
        let name: Vec<u16> = section.name.encode_utf16().collect();
        if name.len() > SECTION_NAME_LEN / 2 {
            return Err(Error::Unwritable(format!(
                "section name {:?} is too long",
                section.name
            )));
        }
        name.iter().for_each(|&u| table.u16(u));
        table.bytes(&[0; SECTION_NAME_LEN][name.len() * 2..]);
        table.u64(w.0.len() as u64);
        table.u64(stored.len() as u64);
        table.u32(section.flags);
        table.u32(section.unknown);
        table.u64(payload.len() as u64);
        w.bytes(&stored);
    }
    w.bytes(&table.0);
    out.write_all(&w.0)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::build_container;
    use std::io;

    #[test]
    fn writes_what_it_reads() {
        let payload = vec![7u8; 1000];
        let bytes = build_container(&[("RAW", b"abcd", false), ("PACKED", &payload, true)]);
        let mut c = Container::new(io::Cursor::new(&bytes)).unwrap();
        let sections = c.sections.clone();
        let payloads: Vec<_> = sections
            .iter()
            .map(|s| c.read_section(s).unwrap())
            .collect();
        let pairs: Vec<_> = sections
            .iter()
            .zip(payloads.iter().map(Vec::as_slice))
            .collect();
        let mut out = Vec::new();
        write(&mut out, &pairs).unwrap();
        assert_eq!(out, bytes, "laid out as the game does");
    }

    #[test]
    fn takes_compression_from_the_flag() {
        // Some payloads deflate to their own size, as MISC_DATA does in a few
        // shipped maps; find one.
        let payload = (1..64)
            .map(|n| (0..n).map(|i| (i * 7 % 5) as u8).collect::<Vec<_>>())
            .find(|p| miniz_oxide::deflate::compress_to_vec_zlib(p, ZLIB_LEVEL).len() == p.len())
            .expect("a payload that deflates to its own size");
        let bytes = build_container(&[("PACKED", &payload, true), ("RAW", &payload, false)]);
        let mut c = Container::new(io::Cursor::new(&bytes)).unwrap();
        let sections = c.sections.clone();
        assert_eq!(sections[0].stored_size, sections[0].unpacked_size);
        assert!(sections[0].is_compressed() && !sections[1].is_compressed());
        for s in &sections {
            assert_eq!(c.read_section(s).unwrap(), payload);
        }
    }
}
