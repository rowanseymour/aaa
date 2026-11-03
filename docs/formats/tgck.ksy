meta:
  id: tgck
  title: TGCK section container
  application: Axis & Allies (2004)
  license: MIT
  endian: le
doc: |
  The container `.rwd` archives and `.rmp` maps share: a header, named
  section payloads back to back, and a table locating each payload at the
  end of the file. A section may be zlib-compressed; its record's flags say
  so, not its sizes, as some payloads deflate to their own size.

  This spec reads any TGCK file with each payload as bytes. `rwd.ksy` and
  `rmp.ksy` reuse its types and give the payloads their layouts.
doc-ref: README.md
seq:
  - id: header
    type: header
  - id: payloads
    size: _io.size - _io.pos - header.section_count * 0x60
    doc: Every section's stored bytes, in the order of the table.
  - id: sections
    type: section_record
    repeat: expr
    repeat-expr: header.section_count
types:
  header:
    seq:
      - id: magic
        contents: TGCK
      - id: version
        type: u4
        valid: 2
      - id: section_count
        type: u4
  section_record:
    seq:
      - id: name
        type: section_name
        size: 0x40
      - id: offset
        type: u8
        doc: From the start of the file.
      - id: len_stored
        type: u8
      - id: flags
        type: u4
        doc: |
          Bit 0 set: the payload is zlib-compressed. The game ignores the
          other bits, which hold leftover memory.
      - id: unknown
        type: u4
        doc: Ignored by the game; leftover memory.
      - id: len_unpacked
        type: u8
        doc: The payload's size once inflated; the stored size if raw.
    instances:
      is_compressed:
        value: flags & 1 != 0
      stored:
        pos: offset
        size: len_stored
      inflated:
        pos: offset
        size: len_stored
        process: zlib
        if: is_compressed
  section_name:
    doc: UTF-16LE, NUL-terminated within its 64 bytes.
    seq:
      - id: units
        type: u2
        repeat: until
        repeat-until: _ == 0
      - id: padding
        size-eos: true
        doc: Zeros, or leftover memory.
    instances:
      value:
        pos: 0
        size: (units.size - 1) * 2
        type: str
        encoding: UTF-16LE
