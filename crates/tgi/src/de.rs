//! Deserializes a block into any type implementing [`serde::Deserialize`].
//!
//! ```text
//! [Terrain]                        struct Terrain {
//! {                                    ids: String,
//!     IDS = temperate_grass            base_priority: i32,
//!     base_priority = 3                terrain_color: Option<[f32; 3]>,
//!     terrain_color = 0.4, 0.6, 0.3    variation: Vec<Variation>,
//!     [Variation] { ... }          }
//!     [Variation] { ... }
//! }
//! ```
//!
//! A key matches a struct field ignoring case and underscores, as the game
//! does with case and the files are inconsistent about both (`IDS`,
//! `DepthInfo` for `depth_info`). Fields and blocks are matched the same way,
//! a block by its kind. A map's keys ignore only case, since they are the
//! file's own names. Where a key repeats, a sequence field takes every
//! occurrence and any other field the first, like [`Block::value`].
//!
//! Values parse from their text: numbers, `true`/`false` (or `1`/`0`), unit
//! enum variants by name (matched like keys), and tuples or arrays from comma
//! separated lists. Unknown keys are skipped unless the type denies them. A
//! float with a stray second decimal part, such as `1.5.0`, reads as far as
//! it's a number, as the game does.
//!
//! With [`Templates::deserialize`] every block is resolved against its
//! template before its contents are read, nested blocks included.
//!
//! [`Coverage`] deserializes the same way while counting which keys the
//! types read and which they skip.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use serde::de::{self, DeserializeOwned, IntoDeserializer, Visitor};

use crate::{Block, Field, Item, Templates};

/// Why a block didn't deserialize, with the line it happened at if known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for Error {}

impl de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error {
            line: None,
            message: msg.to_string(),
        }
    }
}

impl Error {
    /// Attributes an error without a line to `line`.
    fn at(mut self, line: usize) -> Self {
        self.line.get_or_insert(line);
        self
    }
}

/// Deserializes a block's contents as written.
pub fn from_block<T: DeserializeOwned>(block: &Block) -> Result<T, Error> {
    T::deserialize(Node::Block(block, Ctx::top(block, None, None)))
}

impl Templates<'_> {
    /// Deserializes a block's contents with everything it inherits merged in.
    pub fn deserialize<T: DeserializeOwned>(&self, block: &Block) -> Result<T, Error> {
        T::deserialize(Node::Block(block, Ctx::top(block, Some(self), None)))
    }
}

/// How often each key was read or skipped, by the lower-case kinds of the
/// blocks it is in from the top-level block down, e.g. `thing/view/aspect`.
/// Only fields with values count, and template slots in schema files
/// (`[list Property]`, `[optional OnDeath]`) don't.
#[derive(Debug, Default, Clone)]
pub struct Coverage {
    keys: BTreeMap<(String, String), Count>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Count {
    pub read: usize,
    pub unread: usize,
}

impl Coverage {
    /// Deserializes a block, with its inheritance resolved if `templates` are
    /// given, counting the keys the type reads and skips. Keys counted before
    /// an error stay counted.
    pub fn read<T: DeserializeOwned>(
        &mut self,
        block: &Block,
        templates: Option<&Templates>,
    ) -> Result<T, Error> {
        let cell = RefCell::new(std::mem::take(self));
        let value = T::deserialize(Node::Block(block, Ctx::top(block, templates, Some(&cell))));
        *self = cell.into_inner();
        value
    }

    /// Counts every key of a block that nothing reads.
    pub fn skip(&mut self, block: &Block, templates: Option<&Templates>) {
        let path = block.kind().unwrap_or("").to_ascii_lowercase();
        self.unread(&path, block, templates);
    }

    /// `(path, key, count)` for every key seen, in order.
    pub fn keys(&self) -> impl Iterator<Item = (&str, &str, Count)> {
        self.keys
            .iter()
            .map(|((p, k), c)| (p.as_str(), k.as_str(), *c))
    }

    fn count(&mut self, path: &str, field: &Field, read: bool) {
        if field.value.is_none() {
            return;
        }
        let count = self
            .keys
            .entry((path.to_string(), field.key.to_ascii_lowercase()))
            .or_default();
        match read {
            true => count.read += 1,
            false => count.unread += 1,
        }
    }

    fn unread(&mut self, path: &str, block: &Block, templates: Option<&Templates>) {
        let block = &*resolved(templates, block);
        for item in &block.items {
            match item {
                Item::Field(f) => self.count(path, f, false),
                Item::Block(b) if !is_slot(b) => {
                    self.unread(&child_path(path, b), b, templates);
                }
                Item::Block(_) => {}
            }
        }
    }
}

/// Whether a block declares a template slot rather than holding data.
fn is_slot(block: &Block) -> bool {
    block
        .kind()
        .is_some_and(|k| k.eq_ignore_ascii_case("list") || k.eq_ignore_ascii_case("optional"))
}

fn child_path(path: &str, block: &Block) -> String {
    format!("{path}/{}", block.kind().unwrap_or("").to_ascii_lowercase())
}

/// What deserializing carries down into nested blocks.
#[derive(Clone)]
struct Ctx<'a> {
    templates: Option<&'a Templates<'a>>,
    coverage: Option<&'a RefCell<Coverage>>,
    /// The coverage path of the block being read, or of the block holding
    /// the fields being read.
    path: Rc<str>,
}

impl<'a> Ctx<'a> {
    fn top(
        block: &Block,
        templates: Option<&'a Templates<'a>>,
        coverage: Option<&'a RefCell<Coverage>>,
    ) -> Self {
        Ctx {
            templates,
            coverage,
            path: block.kind().unwrap_or("").to_ascii_lowercase().into(),
        }
    }

    fn child(&self, block: &Block) -> Self {
        Ctx {
            path: child_path(&self.path, block).into(),
            ..self.clone()
        }
    }
}

/// Folds case and underscores, so `DepthInfo`, `depthinfo` and `depth_info`
/// are the same key.
fn same_key(a: &str, b: &str) -> bool {
    let fold = |s: &str| {
        s.bytes()
            .filter(|&c| c != b'_')
            .map(|c| c.to_ascii_lowercase())
            .collect::<Vec<_>>()
    };
    fold(a) == fold(b)
}

fn key(item: &Item) -> &str {
    match item {
        Item::Field(f) => &f.key,
        Item::Block(b) => b.kind().unwrap_or(""),
    }
}

/// Something being deserialized.
#[derive(Clone)]
enum Node<'a> {
    Block(&'a Block, Ctx<'a>),
    Field(&'a Field),
    /// The items of a block sharing a key, at least one.
    Group(Vec<&'a Item>, Ctx<'a>),
}

impl<'a> Node<'a> {
    /// An item of the block `ctx` is reading.
    fn item(item: &'a Item, ctx: &Ctx<'a>) -> Self {
        match item {
            Item::Field(f) => Node::Field(f),
            Item::Block(b) => Node::Block(b, ctx.child(b)),
        }
    }

    fn line(&self) -> usize {
        match self {
            Node::Block(b, _) => b.line,
            Node::Field(f) => f.line,
            Node::Group(items, _) => match items[0] {
                Item::Field(f) => f.line,
                Item::Block(b) => b.line,
            },
        }
    }

    /// The single node a scalar or struct is read from: a group's first item.
    /// The group's other items go unread.
    fn first(self) -> Self {
        match self {
            Node::Group(items, ctx) => {
                tally(&items, &ctx, 1);
                Node::item(items[0], &ctx)
            }
            n => n,
        }
    }

    /// Whether the node read as a scalar is a typed declaration without a
    /// value.
    fn is_declaration(&self) -> bool {
        let field = match self {
            Node::Field(f) => Some(*f),
            Node::Group(items, _) => match items[0] {
                Item::Field(f) => Some(f),
                Item::Block(_) => None,
            },
            Node::Block(..) => None,
        };
        field.is_some_and(|f| f.value.is_none())
    }

    /// A field's value, for parsing into a scalar.
    fn text(self, expected: &str) -> Result<(&'a str, usize), Error> {
        match self.first() {
            Node::Field(Field {
                value: Some(v),
                line,
                ..
            }) => Ok((v, *line)),
            n => Err(Error {
                line: Some(n.line()),
                message: format!("expected {expected}, found {}", n.describe()),
            }),
        }
    }

    fn describe(&self) -> String {
        match self {
            Node::Block(b, _) => format!("block [{}]", b.kind().unwrap_or("")),
            Node::Field(f) if f.value.is_none() => format!("{} without a value", f.key),
            Node::Field(_) => "a value".into(),
            Node::Group(..) => "repeated keys".into(),
        }
    }

    fn parse<T>(self, expected: &str, read: fn(&str) -> Option<T>) -> Result<T, Error> {
        let (text, line) = self.text(expected)?;
        read(text.trim()).ok_or_else(|| Error {
            line: Some(line),
            message: format!("expected {expected}, found {text:?}"),
        })
    }

    /// Visits a block's items as a map, keyed by the matching name in
    /// `fields` where there is one. A struct's `fields` are all it reads; a
    /// map (`None`) reads everything.
    fn visit_block<'de, V: Visitor<'de>>(
        self,
        fields: Option<&[&str]>,
        visitor: V,
    ) -> Result<V::Value, Error> {
        let (block, ctx) = match self.first() {
            Node::Block(b, ctx) => (b, ctx),
            n => {
                return Err(Error {
                    line: Some(n.line()),
                    message: format!("expected a block, found {}", n.describe()),
                });
            }
        };
        let block = &*resolved(ctx.templates, block);
        // Only a struct's own field names fold underscores: a map's keys are
        // what the file says, and `a_b` and `ab` are different ones.
        let same = |a: &str, b: &str| match fields {
            Some(_) => same_key(a, b),
            None => a.eq_ignore_ascii_case(b),
        };
        let mut groups: Vec<(&str, Vec<&Item>)> = Vec::new();
        for item in &block.items {
            let k = key(item);
            match groups.iter_mut().find(|(g, _)| same(g, k)) {
                Some((_, items)) => items.push(item),
                None => groups.push((k, vec![item])),
            }
        }
        // A key both declared and given a value reads as its values.
        for (_, items) in &mut groups {
            let valued = |i: &&Item| !matches!(i, Item::Field(Field { value: None, .. }));
            if items.iter().any(valued) {
                items.retain(valued);
            }
        }
        let name = |k: &str| fields.and_then(|f| f.iter().find(|f| same_key(f, k)).copied());
        // Groups that are read are counted as they're read, when it's known
        // whether all of them or only the first is.
        for (k, items) in &groups {
            if fields.is_some() && name(k).is_none() {
                tally(items, &ctx, 0);
            }
        }
        let groups = groups.into_iter().map(|(k, items)| {
            let key = name(k).unwrap_or(k).to_string();
            (key, Node::Group(items, ctx.clone()))
        });
        // The resolved block lives only for this call, so the map access
        // owns what it hands out.
        let groups: Vec<(String, Node<'_>)> = groups.collect();
        let line = block.line;
        let mut map = de::value::MapDeserializer::new(groups.into_iter());
        let value = visitor.visit_map(&mut map).map_err(|e| e.at(line))?;
        map.end().map_err(|e| e.at(line))?;
        Ok(value)
    }
}

impl<'de> IntoDeserializer<'de, Error> for Node<'_> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

macro_rules! parse_scalars {
    ($($method:ident $visit:ident $ty:ty, $expected:literal, $read:ident;)*) => {
        $(fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            visitor.$visit(self.parse::<$ty>($expected, $read)?)
        })*
    };
}

impl<'de> de::Deserializer<'de> for Node<'_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Node::Group(ref items, _) if items.len() > 1 => self.deserialize_seq(visitor),
            n => match n.first() {
                Node::Field(Field { value: None, .. }) => visitor.visit_unit(),
                Node::Field(f) => visitor.visit_str(f.value.as_deref().unwrap_or_default()),
                n => n.deserialize_map(visitor),
            },
        }
    }

    parse_scalars! {
        deserialize_i8 visit_i8 i8, "an integer", integer;
        deserialize_i16 visit_i16 i16, "an integer", integer;
        deserialize_i32 visit_i32 i32, "an integer", integer;
        deserialize_i64 visit_i64 i64, "an integer", integer;
        deserialize_u8 visit_u8 u8, "an integer", integer;
        deserialize_u16 visit_u16 u16, "an integer", integer;
        deserialize_u32 visit_u32 u32, "an integer", integer;
        deserialize_u64 visit_u64 u64, "an integer", integer;
        deserialize_f32 visit_f32 f32, "a number", float;
        deserialize_f64 visit_f64 f64, "a number", float;
        deserialize_char visit_char char, "a character", whole;
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let (text, line) = self.text("true or false")?;
        match parse_bool(text) {
            Some(b) => visitor.visit_bool(b),
            None => Err(Error {
                line: Some(line),
                message: format!("expected true or false, found {text:?}"),
            }),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_str(self.text("a value")?.0)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_bytes(self.text("a value")?.0.as_bytes())
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_bytes(visitor)
    }

    /// Present, since a missing key never reaches the deserializer; a typed
    /// declaration without a value is `None`.
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        if self.is_declaration() {
            self.first();
            return visitor.visit_none();
        }
        visitor.visit_some(self)
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.first();
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    /// Every item of a repeated key.
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let items = match self {
            Node::Group(items, ctx) => {
                tally(&items, &ctx, items.len());
                items.into_iter().map(|i| Node::item(i, &ctx)).collect()
            }
            n => vec![n],
        };
        let mut seq = de::value::SeqDeserializer::new(items.into_iter());
        let value = visitor.visit_seq(&mut seq)?;
        seq.end()?;
        Ok(value)
    }

    /// A comma separated list, e.g. `0.451, 0.604, 0.294`.
    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Error> {
        let (text, line) = self.text("a list")?;
        let parts: Vec<&str> = text.split(',').map(str::trim).collect();
        if parts.len() != len {
            return Err(Error {
                line: Some(line),
                message: format!("expected {len} values, found {text:?}"),
            });
        }
        let mut seq = de::value::SeqDeserializer::new(parts.into_iter().map(Scalar));
        let value = visitor.visit_seq(&mut seq).map_err(|e| e.at(line))?;
        seq.end()?;
        Ok(value)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.visit_block(None, visitor)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.visit_block(Some(fields), visitor)
    }

    /// A unit variant named by the value.
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        let (text, line) = self.text("a name")?;
        let text = text.trim();
        let name = variants
            .iter()
            .find(|v| same_key(v, text))
            .copied()
            .unwrap_or(text);
        visitor
            .visit_enum(name.into_deserializer())
            .map_err(|e: Error| e.at(line))
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_str(visitor)
    }

    /// Unknown keys, which [`Node::visit_block`] has already counted.
    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }
}

/// Counts the items of a group of the block `ctx` is reading, the first
/// `read` of them as read. Blocks that are read count their own keys.
fn tally(items: &[&Item], ctx: &Ctx, read: usize) {
    let Some(coverage) = ctx.coverage else { return };
    let mut coverage = coverage.borrow_mut();
    for (i, item) in items.iter().enumerate() {
        match item {
            Item::Field(f) => coverage.count(&ctx.path, f, i < read),
            Item::Block(b) if i >= read && !is_slot(b) => {
                coverage.unread(&child_path(&ctx.path, b), b, ctx.templates);
            }
            Item::Block(_) => {}
        }
    }
}

/// One element of a comma separated list.
struct Scalar<'a>(&'a str);

impl<'de> IntoDeserializer<'de, Error> for Scalar<'_> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'de> de::Deserializer<'de> for Scalar<'_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_str(self.0)
    }

    parse_scalars! {
        deserialize_i8 visit_i8 i8, "an integer", integer;
        deserialize_i16 visit_i16 i16, "an integer", integer;
        deserialize_i32 visit_i32 i32, "an integer", integer;
        deserialize_i64 visit_i64 i64, "an integer", integer;
        deserialize_u8 visit_u8 u8, "an integer", integer;
        deserialize_u16 visit_u16 u16, "an integer", integer;
        deserialize_u32 visit_u32 u32, "an integer", integer;
        deserialize_u64 visit_u64 u64, "an integer", integer;
        deserialize_f32 visit_f32 f32, "a number", float;
        deserialize_f64 visit_f64 f64, "a number", float;
        deserialize_char visit_char char, "a character", whole;
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match parse_bool(self.0) {
            Some(b) => visitor.visit_bool(b),
            None => Err(de::Error::custom(format!(
                "expected true or false, found {:?}",
                self.0
            ))),
        }
    }

    serde::forward_to_deserialize_any! {
        i128 u128 str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier ignored_any
    }
}

/// The block with what it inherits merged in, where it inherits anything.
fn resolved<'b>(templates: Option<&Templates>, block: &'b Block) -> Cow<'b, Block> {
    match templates {
        Some(t) if t.parent(block).is_some() => Cow::Owned(t.resolve(block)),
        _ => Cow::Borrowed(block),
    }
}

fn whole<T: std::str::FromStr>(text: &str) -> Option<T> {
    text.parse().ok()
}

/// A whole number, where a `%` makes no difference.
fn integer<T: std::str::FromStr>(text: &str) -> Option<T> {
    whole(&text.replace('%', ""))
}

/// A number that may have a fraction: as [`whole`] reads it, divided by 100
/// where the text has a `%`. Where `text` is a number followed by more
/// digits and points, as in the typo `1.5.0`, the number, which is what the
/// game reads. Other trailing text, such as a unit, still fails.
fn float<T: Fraction>(text: &str) -> Option<T> {
    let percent = text.contains('%');
    let text = text.replace('%', "");
    let text = text.trim();
    // Rust reads `inf`, `infinity` and `nan`; the game reads them as 0.
    if text.contains(|c: char| c.is_ascii_alphabetic() && !matches!(c, 'e' | 'E')) {
        return None;
    }
    let n: T = whole(text).or_else(|| {
        let digits = |t: &str| t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
        let sign = usize::from(text.starts_with(['+', '-']));
        let mut end = sign + digits(&text[sign..]);
        if text[end..].starts_with('.') {
            end += 1 + digits(&text[end + 1..]);
        }
        let rest = &text[end..];
        let typo = rest.starts_with('.') && rest.chars().all(|c| c == '.' || c.is_ascii_digit());
        if typo { whole(&text[..end]) } else { None }
    })?;
    Some(if percent { n.hundredth() } else { n })
}

/// A number type with fractions.
trait Fraction: std::str::FromStr {
    fn hundredth(self) -> Self;
}

impl Fraction for f32 {
    fn hundredth(self) -> Self {
        self / 100.0
    }
}

impl Fraction for f64 {
    fn hundredth(self) -> Self {
        self / 100.0
    }
}

/// `true` or `false`, in any case, or `1` or `0`.
fn parse_bool(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

impl Scalar<'_> {
    fn parse<T>(&self, expected: &str, read: fn(&str) -> Option<T>) -> Result<T, Error> {
        read(self.0)
            .ok_or_else(|| de::Error::custom(format!("expected {expected}, found {:?}", self.0)))
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::parse;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Terrain {
        ids: String,
        #[serde(default)]
        base_priority: i32,
        terrain_color: Option<[f32; 3]>,
        #[serde(default)]
        variation: Vec<Variation>,
        depth_info: Option<DepthInfo>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Variation {
        texture: String,
        #[serde(default)]
        blend: bool,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct DepthInfo {
        depth: f32,
        kind: Kind,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    enum Kind {
        Shallow,
        DeepWater,
    }

    fn first(text: &str) -> Block {
        parse(text).root.blocks().next().unwrap().clone()
    }

    #[test]
    fn matches_keys_ignoring_case_and_underscores() {
        let block = first(
            "[Terrain]\n{\nIDS = grass\nBase_Priority = 3\nterrain_color = 0.5, 1, 0.25\n\
             unknown = x\n[Variation]\ntexture = a.dds\nblend = TRUE\n[Variation]\ntexture = b.dds\n\
             [DepthInfo]\ndepth = 2\nkind = deep_water\n}",
        );
        let t: Terrain = from_block(&block).unwrap();
        assert_eq!(
            t,
            Terrain {
                ids: "grass".into(),
                base_priority: 3,
                terrain_color: Some([0.5, 1.0, 0.25]),
                variation: vec![
                    Variation {
                        texture: "a.dds".into(),
                        blend: true
                    },
                    Variation {
                        texture: "b.dds".into(),
                        blend: false
                    },
                ],
                depth_info: Some(DepthInfo {
                    depth: 2.0,
                    kind: Kind::DeepWater
                }),
            }
        );
    }

    #[test]
    fn reads_lists_of_any_scalar() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Mixed {
            list: (bool, u8, i64, char),
        }
        let block = first("[Mixed]\nlist = TRUE, 7, -3, x\n");
        let m: Mixed = from_block(&block).unwrap();
        assert_eq!(m.list, (true, 7, -3, 'x'));
    }

    #[test]
    fn reads_numbers_as_far_as_they_go() {
        #[derive(Debug, Deserialize)]
        struct Info {
            offset: [f32; 2],
            depth: f64,
            count: Option<u8>,
        }
        let block = first("[Info]\noffset = 2.5, 1.5.0\ndepth = -3.25.1.0\n");
        let i: Info = from_block(&block).unwrap();
        assert_eq!((i.offset, i.depth, i.count), ([2.5, 1.5], -3.25, None));

        // Other trailing text is no typo, and words aren't infinities.
        for depth in ["3m", "Infantry", "nano", "inf", "-inf", "infinity", "NaN"] {
            let block = first(&format!("[Info]\noffset = 0, 0\ndepth = {depth}\n"));
            assert!(from_block::<Info>(&block).is_err(), "{depth}");
        }

        // Integers stay whole, so one too big isn't cut short.
        let block = first("[Info]\noffset = 0, 0\ndepth = 0\ncount = 300\n");
        assert!(from_block::<Info>(&block).is_err());
        let block = first("[Info]\noffset = x, 0\ndepth = 0\n");
        assert!(from_block::<Info>(&block).is_err());
    }

    #[test]
    fn percents_divide_only_fractions() {
        #[derive(Debug, Deserialize)]
        struct Sound {
            volume: f32,
            pitch: [f32; 2],
            count: u32,
        }
        let read = |text: &str| from_block::<Sound>(&first(text)).unwrap();
        let s = read("[Sound]\nvolume = 80%\npitch = -5%, 10\ncount = 3%\n");
        assert_eq!((s.volume, s.pitch, s.count), (0.8, [-0.05, 10.0], 3));
        let s = read("[Sound]\nvolume = 80\npitch = 0.5, 1.5.0%\ncount = 3\n");
        assert_eq!((s.volume, s.pitch, s.count), (80.0, [0.5, 0.015], 3));
    }

    #[test]
    fn takes_the_first_of_a_repeated_key() {
        let block = first("[Terrain]\nids = a\nids = b\n");
        let t: Terrain = from_block(&block).unwrap();
        assert_eq!(t.ids, "a");
        assert_eq!(t.terrain_color, None);
    }

    #[test]
    fn reports_the_line() {
        let block = first("[Terrain]\n{\nids = a\nbase_priority = high\n}");
        let err = from_block::<Terrain>(&block).unwrap_err();
        assert_eq!(
            err.to_string(),
            "line 4: expected an integer, found \"high\""
        );

        let block = first("[Terrain]\n{\nterrain_color = 1, 2\n}");
        let err = from_block::<Terrain>(&block).unwrap_err();
        assert_eq!(err.to_string(), "line 3: expected 3 values, found \"1, 2\"");

        let block = first("\n[Terrain]\n{\n}");
        let err = from_block::<Terrain>(&block).unwrap_err();
        assert_eq!(err.to_string(), "line 2: missing field `ids`");
    }

    #[test]
    fn resolves_templates_of_nested_blocks() {
        let parsed = parse(
            "[Template Base]\n{\nids = base\nbase_priority = 1\n[Variation Template=Tex]\n}\n\
             [Template Tex]\n{\ntexture = t.dds\n}\n\
             [Terrain Template=Base]\n{\nbase_priority = 2\n}",
        );
        let templates = Templates::new([&parsed.root]);
        let terrain = parsed.root.blocks_of("Terrain").next().unwrap();
        let t: Terrain = templates.deserialize(terrain).unwrap();
        assert_eq!(t.ids, "base");
        assert_eq!(t.base_priority, 2);
        assert_eq!(
            t.variation,
            vec![Variation {
                texture: "t.dds".into(),
                blend: false
            }]
        );
        assert!(from_block::<Terrain>(terrain).is_err());
    }

    #[test]
    fn counts_read_and_skipped_keys() {
        let parsed = parse(
            "[Template Tex]\n{\ntexture = t.dds\nshine = 1\n}\n\
             [Terrain]\n{\nids = a\ncolour = red\ndeclared\n[Variation Template=Tex]\n\
             [Sound]\n{\nfile = x.wav\n[list Slot]\n{\ndefault = 1\n}\n}\n}",
        );
        let templates = Templates::new([&parsed.root]);
        let terrain = parsed.root.blocks_of("Terrain").next().unwrap();
        let mut coverage = Coverage::default();
        coverage.read::<Terrain>(terrain, Some(&templates)).unwrap();
        coverage.skip(terrain, None);
        let count = |read, unread| Count { read, unread };
        assert_eq!(
            coverage.keys().collect::<Vec<_>>(),
            vec![
                ("terrain", "colour", count(0, 2)),
                ("terrain", "ids", count(1, 1)),
                ("terrain/sound", "file", count(0, 2)),
                ("terrain/variation", "shine", count(0, 1)),
                ("terrain/variation", "texture", count(1, 0)),
            ]
        );
    }

    #[test]
    fn map_keys_keep_their_underscores() {
        let block = first(
            "[Text]\nhires_texas__name = Cannon\nhires_texas_name = Texas\nHIRES_TEXAS_NAME = again\n",
        );
        let text: BTreeMap<String, String> = from_block(&block).unwrap();
        assert_eq!(
            text.into_iter().collect::<Vec<_>>(),
            [
                ("hires_texas__name".to_string(), "Cannon".to_string()),
                ("hires_texas_name".to_string(), "Texas".to_string()),
            ]
        );
    }

    #[test]
    fn counts_only_the_first_of_a_repeated_key_a_field_takes() {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct T {
            ids: String,
            depth_info: Option<DepthInfo>,
        }
        let block = first(
            "[T]\n{\nids = a\nids = b\n[DepthInfo]\ndepth = 1\nkind = Shallow\n\
             [DepthInfo]\ndepth = 2\nkind = Shallow\nextra = 3\n}",
        );
        let mut coverage = Coverage::default();
        coverage.read::<T>(&block, None).unwrap();
        let count = |read, unread| Count { read, unread };
        assert_eq!(
            coverage.keys().collect::<Vec<_>>(),
            vec![
                ("t", "ids", count(1, 1)),
                ("t/depthinfo", "depth", count(1, 1)),
                ("t/depthinfo", "extra", count(0, 1)),
                ("t/depthinfo", "kind", count(1, 1)),
            ]
        );
    }

    #[test]
    fn a_declared_key_reads_its_later_values() {
        #[derive(Deserialize)]
        struct T {
            v: Option<Vec<i32>>,
            w: Option<i32>,
        }
        let t: T = from_block(&first("[T]\nint v\nint v = 3\nint v = 4\nint w\n")).unwrap();
        assert_eq!((t.v, t.w), (Some(vec![3, 4]), None));
    }
}
