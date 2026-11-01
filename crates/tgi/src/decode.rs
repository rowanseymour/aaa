/// Decodes a `.tgi` file: UTF-8 (with or without BOM), falling back to
/// Windows-1252, which the game's Latin-1 files use.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return String::from_utf8_lossy(rest).into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| windows_1252(b)).collect(),
    }
}

fn windows_1252(b: u8) -> char {
    // 0x80..=0x9F differ from Latin-1; everything else maps directly.
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž',
        '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}',
        'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_each_encoding() {
        assert_eq!(decode(b"\xEF\xBB\xBFname = caf\xC3\xA9"), "name = café");
        assert_eq!(decode(b"name = caf\xC3\xA9"), "name = café");
        assert_eq!(decode(b"name = caf\xE9 \x93q\x94"), "name = café “q”");
    }
}
