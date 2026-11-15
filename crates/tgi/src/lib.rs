//! Parser for `.tgi` files, the text data format used by Axis & Allies (2004)
//! for units, templates, maps, localisation and settings.
//!
//! ```text
//! /* block comment */               comments: /* ... */ anywhere, ; to end of line
//! [Thing Template=AAUnitUSAProne]   header: words, then key=value attributes
//! {                                 braces are needed only to nest blocks
//!     IDS = usa_engineer            field: key = value (value runs to end of line)
//!     name = "#usa_engineer_name"   surrounding quotes are stripped
//!     fixed health                  typed field without a value (schema files)
//!     [BodyComponent]               a header without braces owns the fields after it,
//!     health = 200                  up to the next header or closing brace
//!     defense = 1
//! }
//! ```
//!
//! `//` is not a comment (it appears in URLs). The parser is lenient like the
//! game: malformed input produces [`Warning`]s rather than errors.
//!
//! Schema files declare the nested blocks a kind may hold with slots: headers
//! whose first word is `list` or `optional`, followed by the kind, as in
//! `[list Property]` or `[optional OnDeath]`.
//!
//! [`Templates`] resolves template inheritance (`Template=`, `Inherit=`) on
//! request; the tree itself is left as written.
//!
//! With the `serde` feature, `from_block` reads a block into a typed struct.

#[cfg(feature = "serde")]
pub mod de;
mod decode;
mod inherit;

#[cfg(feature = "serde")]
pub use de::{Count, Coverage, Error, from_block};
pub use decode::decode;
pub use inherit::Templates;

/// A block: a header with the fields and blocks it contains. The document root
/// is a block with an empty header.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Block {
    pub header: Header,
    pub items: Vec<Item>,
    /// Whether the body was written in `{ }` (as opposed to INI-style).
    pub braced: bool,
    /// 1-based line of the header (0 for the root).
    pub line: usize,
}

/// What's between a block's `[ ]`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    /// Bare words, e.g. `["Template", "AAUSABattleShip"]` or `["optional", "OnDeathAbility"]`.
    pub words: Vec<String>,
    /// `key=value` attributes in order, e.g. `[("Template", "AAAttackBattleship")]`.
    pub attrs: Vec<(String, String)>,
}

/// A block's content, in the order written.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Field(Field),
    Block(Block),
}

/// A `key = value` line, or a schema declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Type in schema files, e.g. `fixed`.
    pub ty: Option<String>,
    pub key: String,
    /// `None` for declarations without `=`.
    pub value: Option<String>,
    /// Whether the value was written in double quotes.
    pub quoted: bool,
    /// Line, numbered as [`Block::line`].
    pub line: usize,
}

/// A problem the parser read past.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Line, numbered as [`Block::line`].
    pub line: usize,
    pub message: String,
}

/// A parsed document.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub root: Block,
    pub warnings: Vec<Warning>,
}

impl Block {
    /// First header word, e.g. `Thing`, `BodyComponent`, `template`.
    pub fn kind(&self) -> Option<&str> {
        self.header.words.first().map(String::as_str)
    }

    /// Second header word, e.g. the template name in `[Template AAUSATank ...]`.
    pub fn name(&self) -> Option<&str> {
        self.header.words.get(1).map(String::as_str)
    }

    /// Header attribute, case-insensitive (`Template=` and `template=` both occur).
    pub fn attr(&self, key: &str) -> Option<&str> {
        let (_, v) = self
            .header
            .attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))?;
        Some(v)
    }

    /// The fields directly in this block.
    pub fn fields(&self) -> impl Iterator<Item = &Field> {
        self.items.iter().filter_map(|i| match i {
            Item::Field(f) => Some(f),
            Item::Block(_) => None,
        })
    }

    /// The blocks directly in this block.
    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.items.iter().filter_map(|i| match i {
            Item::Block(b) => Some(b),
            Item::Field(_) => None,
        })
    }

    /// First field with this key, case-insensitive.
    pub fn field(&self, key: &str) -> Option<&Field> {
        self.fields().find(|f| f.key.eq_ignore_ascii_case(key))
    }

    /// Value of the first field with this key, case-insensitive.
    pub fn value(&self, key: &str) -> Option<&str> {
        self.field(key)?.value.as_deref()
    }

    /// Value of the first field with this key, parsed, e.g. a number.
    pub fn parse_value<T: std::str::FromStr>(&self, key: &str) -> Option<T> {
        self.value(key)?.trim().parse().ok()
    }

    /// Whether the first field with this key is true, as the game reads
    /// flags: `true` in any case, or `1`.
    pub fn flag(&self, key: &str) -> bool {
        self.value(key).map(str::trim).and_then(parse_bool) == Some(true)
    }

    /// For a schema slot, the kind it declares (`""` if it names none).
    pub(crate) fn slot(&self) -> Option<&str> {
        let k = self.kind()?;
        let slot = k.eq_ignore_ascii_case("list") || k.eq_ignore_ascii_case("optional");
        slot.then(|| self.name().unwrap_or(""))
    }

    /// Child blocks whose first header word matches, case-insensitive.
    pub fn blocks_of<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Block> {
        self.blocks()
            .filter(move |b| b.kind().is_some_and(|k| k.eq_ignore_ascii_case(kind)))
    }
}

/// Decodes and parses a file's raw bytes.
pub fn parse_bytes(bytes: &[u8]) -> Parsed {
    parse(&decode(bytes))
}

/// Parses a file's text.
pub fn parse(text: &str) -> Parsed {
    let mut p = Parser {
        warnings: Vec::new(),
        stack: vec![Frame::root()],
    };
    let text = strip_block_comments(text, &mut p.warnings);
    for (i, line) in text.lines().enumerate() {
        p.line(line, i + 1);
    }
    p.finish()
}

/// Replaces `/* ... */` with spaces, keeping newlines so line numbers survive.
/// One can't start inside quotes or after a `;` comment on its line.
fn strip_block_comments(text: &str, warnings: &mut Vec<Warning>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut line = 1;
    let (mut quoted, mut commented) = (false, false);
    while let Some(c) = rest.chars().next() {
        if c == '\n' {
            line += 1;
            (quoted, commented) = (false, false);
        } else if !commented && c == '"' {
            quoted = !quoted;
        } else if !commented && !quoted && c == ';' {
            commented = true;
        } else if !commented && !quoted && rest.starts_with("/*") {
            // The close is looked for from the opening `*`, so `/*/` is a
            // whole comment, as the game reads it.
            let len = match rest[1..].find("*/") {
                Some(end) => end + 3,
                None => {
                    warnings.push(Warning {
                        line,
                        message: "unterminated /* comment".into(),
                    });
                    rest.len()
                }
            };
            let comment = &rest[..len];
            out.extend(comment.chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
            line += comment.matches('\n').count();
            rest = &rest[len..];
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

struct Frame {
    block: Block,
    /// A header was just read and may still be followed by `{`.
    awaiting_brace: bool,
}

impl Frame {
    fn root() -> Self {
        Frame {
            block: Block::default(),
            awaiting_brace: false,
        }
    }
}

struct Parser {
    warnings: Vec<Warning>,
    /// Open blocks, innermost last; `stack[0]` is the root.
    stack: Vec<Frame>,
}

impl Parser {
    fn warn(&mut self, line: usize, message: impl Into<String>) {
        self.warnings.push(Warning {
            line,
            message: message.into(),
        });
    }

    fn top(&mut self) -> &mut Frame {
        self.stack.last_mut().expect("root frame")
    }

    /// Pops the innermost frame into its parent.
    fn pop(&mut self) {
        let frame = self.stack.pop().expect("non-root frame");
        self.top().block.items.push(Item::Block(frame.block));
    }

    /// Ends an open INI-style section, if any.
    fn close_section(&mut self) {
        while self.stack.len() > 1 && !self.top().block.braced {
            self.pop();
        }
    }

    fn line(&mut self, text: &str, line: usize) {
        let mut rest = text;
        loop {
            rest = rest.trim_start();
            let Some(c) = rest.chars().next() else { return };
            match c {
                ';' => return,
                '{' => {
                    if self.stack.len() > 1 && !self.top().block.braced {
                        // Normally right after the header; after fields it still
                        // most plausibly continues the same section.
                        if !self.top().awaiting_brace {
                            self.warn(
                                line,
                                "'{' after the section's fields; continuing the section",
                            );
                        }
                        let top = self.top();
                        top.block.braced = true;
                        top.awaiting_brace = false;
                    } else {
                        self.warn(line, "'{' without a preceding header");
                        self.close_section();
                        self.stack.push(Frame {
                            block: Block {
                                braced: true,
                                line,
                                ..Block::default()
                            },
                            awaiting_brace: false,
                        });
                    }
                    rest = &rest[1..];
                }
                '}' => {
                    self.close_section();
                    if self.stack.len() > 1 {
                        self.pop();
                    } else {
                        self.warn(line, "unmatched '}'");
                    }
                    rest = &rest[1..];
                }
                '[' => {
                    let (inner, after) = match rest[1..].find(']') {
                        Some(end) => (&rest[1..1 + end], &rest[end + 2..]),
                        None => {
                            self.warn(line, "header missing ']'");
                            (strip_comment(&rest[1..]), "")
                        }
                    };
                    let header = self.header(inner, line);
                    self.close_section();
                    self.stack.push(Frame {
                        block: Block {
                            header,
                            line,
                            ..Block::default()
                        },
                        awaiting_brace: true,
                    });
                    rest = after.trim_start();
                    if !rest.is_empty() && !rest.starts_with(['{', '}', '[', ';']) {
                        self.warn(
                            line,
                            format!("ignoring text after header: {:?}", rest.trim_end()),
                        );
                        return;
                    }
                }
                _ => {
                    let field = self.field(strip_comment(rest), line);
                    let top = self.top();
                    top.awaiting_brace = false;
                    top.block.items.push(Item::Field(field));
                    return;
                }
            }
        }
    }

    fn header(&mut self, inner: &str, line: usize) -> Header {
        let tokens: Vec<&str> = inner.split_whitespace().collect();
        let mut h = Header::default();
        let mut i = 0;
        while i < tokens.len() {
            let t = tokens[i];
            let next = tokens.get(i + 1).copied();
            let after = tokens.get(i + 2).copied();
            if t == "=" {
                self.warn(line, "stray '=' in header");
                i += 1;
            } else if let Some((k, v)) = t.split_once('=') {
                // `key=value`, or `key= value`
                match (v.is_empty(), next) {
                    (true, Some(nv)) if !nv.contains('=') => {
                        h.attrs.push((k.into(), nv.into()));
                        i += 2;
                    }
                    _ => {
                        h.attrs.push((k.into(), v.into()));
                        i += 1;
                    }
                }
            } else if next == Some("=") && after.is_some_and(|v| !v.contains('=')) {
                // `key = value`
                h.attrs.push((t.into(), after.unwrap().into()));
                i += 3;
            } else if let Some(v) = next
                .and_then(|nv| nv.strip_prefix('='))
                .filter(|v| !v.is_empty())
            {
                // `key =value`
                h.attrs.push((t.into(), v.into()));
                i += 2;
            } else {
                h.words.push(t.into());
                i += 1;
            }
        }
        h
    }

    fn field(&mut self, text: &str, line: usize) -> Field {
        let (lhs, value) = match text.split_once('=') {
            Some((l, v)) => (l.trim(), Some(v.trim())),
            None => (text.trim(), None),
        };
        let mut words: Vec<&str> = lhs.split_whitespace().collect();
        if words.is_empty() {
            self.warn(line, "field without a key");
            words.push("");
        }
        if words.len() > 2 {
            self.warn(line, format!("unexpected words before key: {lhs:?}"));
        }
        let key = words.pop().unwrap().to_string();
        let ty = (!words.is_empty()).then(|| words.join(" "));
        let (value, quoted) = match value {
            Some(v) if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') => {
                (Some(v[1..v.len() - 1].to_string()), true)
            }
            v => (v.map(str::to_string), false),
        };
        Field {
            ty,
            key,
            value,
            quoted,
            line,
        }
    }

    fn finish(mut self) -> Parsed {
        self.close_section();
        while self.stack.len() > 1 {
            let line = self.top().block.line;
            self.warn(line, "block not closed before end of file");
            self.pop();
            self.close_section();
        }
        Parsed {
            root: self.stack.pop().unwrap().block,
            warnings: self.warnings,
        }
    }
}

/// `true` or `false`, in any case, or `1` or `0`, already trimmed.
fn parse_bool(text: &str) -> Option<bool> {
    match text {
        t if t == "1" || t.eq_ignore_ascii_case("true") => Some(true),
        t if t == "0" || t.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

/// Cuts a `;` comment, ignoring semicolons inside double quotes.
fn strip_comment(s: &str) -> &str {
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' if !quoted => return &s[..i],
            _ => {}
        }
    }
    s
}

#[cfg(test)]
mod tests;
