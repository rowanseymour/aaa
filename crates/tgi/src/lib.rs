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
//! This crate produces the syntax tree only. Template inheritance (`Template=`,
//! `Inherit=`) is resolved by consumers.

mod decode;

pub use decode::decode;

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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    /// Bare words, e.g. `["Template", "AAUSABattleShip"]` or `["optional", "OnDeathAbility"]`.
    pub words: Vec<String>,
    /// `key=value` attributes in order, e.g. `[("Template", "AAAttackBattleship")]`.
    pub attrs: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Field(Field),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Type in schema files: `fixed`, `string`, `flag`, `int`, `float`, `bool`, `localized`.
    pub ty: Option<String>,
    pub key: String,
    /// `None` for declarations without `=`.
    pub value: Option<String>,
    /// Whether the value was written in double quotes.
    pub quoted: bool,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub line: usize,
    pub message: String,
}

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

    pub fn fields(&self) -> impl Iterator<Item = &Field> {
        self.items.iter().filter_map(|i| match i {
            Item::Field(f) => Some(f),
            Item::Block(_) => None,
        })
    }

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
fn strip_block_comments(text: &str, warnings: &mut Vec<Warning>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut line = 1;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        line += rest[..start].matches('\n').count();
        let after = &rest[start + 2..];
        let (comment, next) = match after.find("*/") {
            Some(end) => (&rest[start..start + 2 + end + 2], &after[end + 2..]),
            None => {
                warnings.push(Warning {
                    line,
                    message: "unterminated /* comment".into(),
                });
                (&rest[start..], "")
            }
        };
        out.extend(comment.chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
        line += comment.matches('\n').count();
        rest = next;
    }
    out.push_str(rest);
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

    fn line(&mut self, line: &str, n: usize) {
        let mut rest = line;
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
                            self.warn(n, "'{' after the section's fields; continuing the section");
                        }
                        let top = self.top();
                        top.block.braced = true;
                        top.awaiting_brace = false;
                    } else {
                        self.warn(n, "'{' without a preceding header");
                        self.close_section();
                        self.stack.push(Frame {
                            block: Block {
                                braced: true,
                                line: n,
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
                        self.warn(n, "unmatched '}'");
                    }
                    rest = &rest[1..];
                }
                '[' => {
                    let (inner, after) = match rest[1..].find(']') {
                        Some(end) => (&rest[1..1 + end], &rest[end + 2..]),
                        None => {
                            self.warn(n, "header missing ']'");
                            (strip_comment(&rest[1..]), "")
                        }
                    };
                    let header = self.header(inner, n);
                    self.close_section();
                    self.stack.push(Frame {
                        block: Block {
                            header,
                            line: n,
                            ..Block::default()
                        },
                        awaiting_brace: true,
                    });
                    rest = after.trim_start();
                    if !rest.is_empty() && !rest.starts_with(['{', '}', '[', ';']) {
                        self.warn(
                            n,
                            format!("ignoring text after header: {:?}", rest.trim_end()),
                        );
                        return;
                    }
                }
                _ => {
                    let field = self.field(strip_comment(rest), n);
                    let top = self.top();
                    top.awaiting_brace = false;
                    top.block.items.push(Item::Field(field));
                    return;
                }
            }
        }
    }

    fn header(&mut self, inner: &str, n: usize) -> Header {
        let tokens: Vec<&str> = inner.split_whitespace().collect();
        let mut h = Header::default();
        let mut i = 0;
        while i < tokens.len() {
            let t = tokens[i];
            let next = tokens.get(i + 1).copied();
            let after = tokens.get(i + 2).copied();
            if t == "=" {
                self.warn(n, "stray '=' in header");
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

    fn field(&mut self, text: &str, n: usize) -> Field {
        let (lhs, value) = match text.split_once('=') {
            Some((l, v)) => (l.trim(), Some(v.trim())),
            None => (text.trim(), None),
        };
        let mut words: Vec<&str> = lhs.split_whitespace().collect();
        if words.is_empty() {
            self.warn(n, "field without a key");
            words.push("");
        }
        if words.len() > 2 {
            self.warn(n, format!("unexpected words before key: {lhs:?}"));
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
            line: n,
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
