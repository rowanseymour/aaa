//! File archives (`.rwd`), laid out as [`rwd.ksy`] specifies.
//!
//! [`rwd.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rwd.ksy

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use crate::container::{Container, Section};
use crate::{Cursor, Error, Result};

#[derive(Debug, Clone)]
pub struct Entry {
    /// Path inside the archive, '/'-separated, e.g. `Audio/Music/WinInterface.mp3`.
    pub name: String,
    /// Absolute offset of the file data within the archive.
    pub offset: u64,
    pub size: u64,
    pub unknown: u32,
    pub index: u32,
}

impl Entry {
    /// A relative filesystem path for this entry, or `None` if the name could
    /// escape the destination directory (absolute, `..`, drive prefixes).
    pub fn safe_path(&self) -> Option<PathBuf> {
        let normalized = self.name.replace('\\', "/");
        let path = Path::new(&normalized);
        let mut out = PathBuf::new();
        for c in path.components() {
            match c {
                Component::Normal(p) => out.push(p),
                Component::CurDir => {}
                _ => return None,
            }
        }
        (!out.as_os_str().is_empty()).then_some(out)
    }
}

/// The archive's `Header` section, which the game reads but doesn't use.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub version: u32,
    pub product: String,
    pub unknown: [String; 2],
    /// From version 2 on.
    pub unknown2: Option<u32>,
}

impl Header {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let version = cur.u32()?;
        let header = Header {
            version,
            product: cur.string()?,
            unknown: [cur.string()?, cur.string()?],
            unknown2: (version >= 2).then(|| cur.u32()).transpose()?,
        };
        cur.finish("archive header")?;
        Ok(header)
    }
}

pub struct Archive<R> {
    container: Container<R>,
    pub entries: Vec<Entry>,
}

impl Archive<BufReader<File>> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::new(BufReader::new(File::open(path)?))
    }
}

impl<R: Read + Seek> Archive<R> {
    pub fn new(reader: R) -> Result<Self> {
        let mut container = Container::new(reader)?;
        let files = container.require("Files")?.clone();
        if files.is_compressed() {
            return Err(Error::Corrupt("compressed Files section".into()));
        }
        let dir = container.read_named("Footer")?;
        let entries = parse_directory(&dir, &files)?;
        Ok(Archive { container, entries })
    }

    pub fn sections(&self) -> &[Section] {
        &self.container.sections
    }

    pub fn header(&mut self) -> Result<Header> {
        Header::parse(&self.container.read_named("Header")?)
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
    }

    pub fn read(&mut self, entry: &Entry) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(entry.size as usize);
        self.copy_to(entry, &mut buf)?;
        Ok(buf)
    }

    /// Streams an entry into `out` without holding it all in memory.
    pub fn copy_to(&mut self, entry: &Entry, out: &mut impl io::Write) -> Result<u64> {
        let reader = &mut self.container.reader;
        reader.seek(SeekFrom::Start(entry.offset))?;
        let copied = io::copy(&mut reader.take(entry.size), out)?;
        if copied != entry.size {
            return Err(Error::Corrupt(format!("{}: short read", entry.name)));
        }
        Ok(copied)
    }
}

fn parse_directory(dir: &[u8], files: &Section) -> Result<Vec<Entry>> {
    let mut cur = Cursor::new(dir);
    let count = cur.u32()?;
    // An entry takes at least 26 bytes.
    let mut entries = Vec::with_capacity((count as usize).min(cur.remaining() / 26));
    for i in 0..count {
        let name = cur.string()?;
        let rel = cur.u64()?;
        let size = cur.u64()?;
        let unknown = cur.u32()?;
        let index = cur.u32()?;
        if rel
            .checked_add(size)
            .is_none_or(|end| end > files.stored_size)
        {
            return Err(Error::Corrupt(format!(
                "entry {i} ({name}) outside Files section"
            )));
        }
        entries.push(Entry {
            name,
            offset: files.offset + rel,
            size,
            unknown,
            index,
        });
    }
    cur.finish("directory")?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{build_container, string};

    fn build(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut dir = (files.len() as u32).to_le_bytes().to_vec();
        for (i, (name, bytes)) in files.iter().enumerate() {
            dir.extend(string(name));
            dir.extend((data.len() as u64).to_le_bytes());
            dir.extend((bytes.len() as u64).to_le_bytes());
            dir.extend(0u32.to_le_bytes());
            dir.extend((i as u32).to_le_bytes());
            data.extend(*bytes);
        }
        let header = [2, 0, 0, 0, 2, 0, b'A', 0, b'A', 0, 0, 0, 0, 0, 0, 0, 0, 0];
        build_container(&[
            ("Header", &header, false),
            ("Files", &data, false),
            ("Footer", &dir, false),
        ])
    }

    #[test]
    fn roundtrip() {
        let bytes = build(&[("A/one.txt", b"hello"), ("B/two.bin", b"\x00\x01\x02")]);
        let mut ar = Archive::new(io::Cursor::new(bytes)).unwrap();
        assert_eq!(ar.entries.len(), 2);
        let header = ar.header().unwrap();
        assert_eq!((header.version, header.product.as_str()), (2, "AA"));
        assert_eq!(header.unknown, [String::new(), String::new()]);
        assert_eq!(header.unknown2, Some(0));
        let e = ar.find("b/TWO.bin").unwrap().clone();
        assert_eq!(ar.read(&e).unwrap(), b"\x00\x01\x02");
        assert_eq!(ar.read(&ar.entries[0].clone()).unwrap(), b"hello");
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = build(&[("x", b"y")]);
        bytes[0] = b'X';
        assert!(matches!(
            Archive::new(io::Cursor::new(bytes)),
            Err(Error::BadMagic(_))
        ));
    }

    #[test]
    fn rejects_entry_outside_files() {
        let mut bytes = build(&[("x", b"y")]);
        // Bump the single entry's size so it runs past the Files section.
        let size_pos = bytes.len() - 3 * 0x60 - 16;
        bytes[size_pos] = 0xff;
        assert!(matches!(
            Archive::new(io::Cursor::new(bytes)),
            Err(Error::Corrupt(_))
        ));
    }

    #[test]
    fn safe_paths() {
        let e = |n: &str| Entry {
            name: n.into(),
            offset: 0,
            size: 0,
            unknown: 0,
            index: 0,
        };
        assert_eq!(
            e("Audio/Music/a.mp3").safe_path(),
            Some(PathBuf::from("Audio/Music/a.mp3"))
        );
        assert_eq!(
            e("Audio\\a.mp3").safe_path(),
            Some(PathBuf::from("Audio/a.mp3"))
        );
        assert_eq!(e("../evil").safe_path(), None);
        assert_eq!(e("/etc/passwd").safe_path(), None);
    }
}
