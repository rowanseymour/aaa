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
//! a block by its kind. Where a key repeats, a sequence field takes every
//! occurrence and any other field the first, like [`Block::value`].
//!
//! Values parse from their text: numbers, `true`/`false` (or `1`/`0`), unit
//! enum variants by name (matched like keys), and tuples or arrays from comma
//! separated lists. Unknown keys are skipped unless the type denies them.
//!
//! With [`Templates::deserialize`] every block is resolved against its
//! template before its contents are read, nested blocks included.

use std::fmt;

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
    T::deserialize(Node::Block(block, None))
}

impl Templates<'_> {
    /// Deserializes a block's contents with everything it inherits merged in.
    pub fn deserialize<T: DeserializeOwned>(&self, block: &Block) -> Result<T, Error> {
        T::deserialize(Node::Block(block, Some(self)))
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
    Block(&'a Block, Option<&'a Templates<'a>>),
    Field(&'a Field),
    /// The items of a block sharing a key, at least one.
    Group(Vec<&'a Item>, Option<&'a Templates<'a>>),
}

impl<'a> Node<'a> {
    fn item(item: &'a Item, templates: Option<&'a Templates<'a>>) -> Self {
        match item {
            Item::Field(f) => Node::Field(f),
            Item::Block(b) => Node::Block(b, templates),
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
    fn first(self) -> Self {
        match self {
            Node::Group(items, templates) => Node::item(items[0], templates),
            n => n,
        }
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

    fn parse<T: std::str::FromStr>(self, expected: &str) -> Result<T, Error> {
        let (text, line) = self.text(expected)?;
        text.trim().parse().map_err(|_| Error {
            line: Some(line),
            message: format!("expected {expected}, found {text:?}"),
        })
    }

    /// Visits a block's items as a map, keyed by the matching name in
    /// `fields` where there is one.
    fn visit_block<'de, V: Visitor<'de>>(
        self,
        fields: &[&str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        let (block, templates) = match self.first() {
            Node::Block(b, t) => (b, t),
            n => {
                return Err(Error {
                    line: Some(n.line()),
                    message: format!("expected a block, found {}", n.describe()),
                });
            }
        };
        let resolved;
        let block = match templates {
            Some(t) if t.parent(block).is_some() => {
                resolved = t.resolve(block);
                &resolved
            }
            _ => block,
        };
        let mut groups: Vec<(&str, Vec<&Item>)> = Vec::new();
        for item in &block.items {
            let k = key(item);
            match groups.iter_mut().find(|(g, _)| same_key(g, k)) {
                Some((_, items)) => items.push(item),
                None => groups.push((k, vec![item])),
            }
        }
        let groups = groups.into_iter().map(|(k, items)| {
            let name = fields.iter().find(|f| same_key(f, k)).copied().unwrap_or(k);
            (name.to_string(), Node::Group(items, templates))
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
    ($($method:ident $visit:ident $ty:ty, $expected:literal;)*) => {
        $(fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            visitor.$visit(self.parse::<$ty>($expected)?)
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
        deserialize_i8 visit_i8 i8, "an integer";
        deserialize_i16 visit_i16 i16, "an integer";
        deserialize_i32 visit_i32 i32, "an integer";
        deserialize_i64 visit_i64 i64, "an integer";
        deserialize_u8 visit_u8 u8, "an integer";
        deserialize_u16 visit_u16 u16, "an integer";
        deserialize_u32 visit_u32 u32, "an integer";
        deserialize_u64 visit_u64 u64, "an integer";
        deserialize_f32 visit_f32 f32, "a number";
        deserialize_f64 visit_f64 f64, "a number";
        deserialize_char visit_char char, "a character";
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let (text, line) = self.text("true or false")?;
        match text.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => visitor.visit_bool(true),
            "false" | "0" => visitor.visit_bool(false),
            _ => Err(Error {
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
        match self.clone().first() {
            Node::Field(Field { value: None, .. }) => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_unit()
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
            Node::Group(items, templates) => items
                .into_iter()
                .map(|i| Node::item(i, templates))
                .collect(),
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
        self.visit_block(&[], visitor)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.visit_block(fields, visitor)
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

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_unit()
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

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_f32(self.parse("a number")?)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_f64(self.parse("a number")?)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_i32(self.parse("an integer")?)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_u32(self.parse("an integer")?)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i64 i128 u8 u16 u64 u128 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier ignored_any
    }
}

impl Scalar<'_> {
    fn parse<T: std::str::FromStr>(&self, expected: &str) -> Result<T, Error> {
        self.0
            .parse()
            .map_err(|_| de::Error::custom(format!("expected {expected}, found {:?}", self.0)))
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
}
