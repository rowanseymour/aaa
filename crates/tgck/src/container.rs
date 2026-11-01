//! The TGCK section container shared by `.rwd` archives and `.rmp` maps.
//!
//! ```text
//! 0x00  "TGCK"  u32 version (2)  u32 section_count
//! 0x0C  section payloads, back to back
//! end   section table: section_count records of 0x60 bytes:
//!         UTF-16LE name[32], u64 offset, u64 stored_size,
//!         8 bytes unknown (constant per file; uninitialised memory in .rwd),
//!         u64 unpacked_size
//! ```
//!
//! A section is zlib-compressed when `stored_size != unpacked_size`.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::{Error, Result, utf16_lossy};

pub const MAGIC: &[u8; 4] = b"TGCK";
const SECTION_RECORD_LEN: u64 = 0x60;
const SECTION_NAME_LEN: usize = 0x40;
/// Refuse to inflate sections claiming more than this; real ones are a few MB.
const MAX_UNPACKED: u64 = 256 << 20;

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub offset: u64,
    pub stored_size: u64,
    pub unpacked_size: u64,
    pub unknown: [u8; 8],
}

impl Section {
    pub fn is_compressed(&self) -> bool {
        self.stored_size != self.unpacked_size
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
        let magic: [u8; 4] = head[..4].try_into().unwrap();
        if &magic != MAGIC {
            return Err(Error::BadMagic(magic));
        }
        let version = u32::from_le_bytes(head[4..8].try_into().unwrap());
        if version != 2 {
            return Err(Error::UnsupportedVersion(version));
        }
        let section_count = u32::from_le_bytes(head[8..12].try_into().unwrap()) as u64;

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
            let u64_at = |o: usize| u64::from_le_bytes(rec[o..o + 8].try_into().unwrap());
            let section = Section {
                name: utf16_lossy(&rec[..SECTION_NAME_LEN]),
                offset: u64_at(0x40),
                stored_size: u64_at(0x48),
                unknown: rec[0x50..0x58].try_into().unwrap(),
                unpacked_size: u64_at(0x58),
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
        let section = self
            .section(name)
            .cloned()
            .ok_or(Error::MissingSection(name))?;
        self.read_section(&section)
    }
}
