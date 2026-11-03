meta:
  id: rwd
  title: Axis & Allies (2004) file archive
  file-extension: rwd
  license: MIT
  endian: le
  imports:
    - tgck
doc: |
  A TGCK container of three sections: `Header`, `Files`, the files' data
  back to back, and `Footer`, the directory locating each file in `Files`.
  The game's data, music, speech and videos ship as archives.
doc-ref: README.md
seq:
  - id: header
    type: tgck::header
  - id: payloads
    size: _io.size - _io.pos - header.section_count * 0x60
  - id: sections
    type: section
    repeat: expr
    repeat-expr: header.section_count
instances:
  archive_header:
    value: sections[archive_header_index.index].archive_header
  archive_header_index:
    pos: 0
    size: 0
    type: find_section('Header', 0)
  files_section:
    value: sections[files_index.index].record
  files_index:
    pos: 0
    size: 0
    type: find_section('Files', 0)
  directory:
    value: sections[directory_index.index].directory
  directory_index:
    pos: 0
    size: 0
    type: find_section('Footer', 0)
types:
  section:
    seq:
      - id: record
        type: tgck::section_record
    instances:
      archive_header:
        value: 'record.is_compressed ? inflated_archive_header : stored_archive_header'
        if: record.name.value == 'Header'
      stored_archive_header:
        pos: record.offset
        size: record.len_stored
        type: archive_header
        if: record.name.value == 'Header' and not record.is_compressed
      inflated_archive_header:
        pos: record.offset
        size: record.len_stored
        process: zlib
        type: archive_header
        if: record.name.value == 'Header' and record.is_compressed
      directory:
        value: 'record.is_compressed ? inflated_directory : stored_directory'
        if: record.name.value == 'Footer'
      stored_directory:
        pos: record.offset
        size: record.len_stored
        type: directory
        if: record.name.value == 'Footer' and not record.is_compressed
      inflated_directory:
        pos: record.offset
        size: record.len_stored
        process: zlib
        type: directory
        if: record.name.value == 'Footer' and record.is_compressed
  find_section:
    doc: The index of the first section named `name` at or after `i`.
    params:
      - id: name
        type: str
      - id: i
        type: u4
    instances:
      index:
        value: '(_root.sections[i].record.name.value == name ? i : next.index).as<u4>'
      next:
        pos: 0
        size: 0
        type: find_section(name, i + 1)
        if: _root.sections[i].record.name.value != name
  archive_header:
    doc: The `Header` section. The game reads it but uses none of it.
    seq:
      - id: version
        type: u4
        doc: 2 in every shipped archive.
      - id: product
        type: string
        doc: '`AA` in every shipped archive.'
      - id: unknown1
        type: string
        doc: Unknown; empty in every shipped archive.
      - id: unknown2
        type: string
        doc: Unknown; empty in every shipped archive.
      - id: unknown3
        type: u4
        if: version >= 2
        doc: Unknown; differs between archives, with no pattern.
  directory:
    doc: The `Footer` section. `Files` is always stored raw.
    seq:
      - id: num_entries
        type: u4
      - id: entries
        type: entry
        repeat: expr
        repeat-expr: num_entries
  entry:
    seq:
      - id: name
        type: string
        doc: Its path, `/`-separated, e.g. `Audio/Music/WinInterface.mp3`.
      - id: offset
        type: u8
        doc: From the start of the `Files` section.
      - id: len_data
        type: u8
      - id: unknown
        type: u4
        doc: 0 in every shipped archive.
      - id: index
        type: u4
        doc: The file's position in data order.
    instances:
      data:
        io: _root._io
        pos: _root.files_section.offset + offset
        size: len_data
  string:
    seq:
      - id: len
        type: u2
        doc: In UTF-16 code units.
      - id: value
        type: str
        size: len * 2
        encoding: UTF-16LE
