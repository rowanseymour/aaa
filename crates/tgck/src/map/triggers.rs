//! TRIGGERS: the map's scripting. Each trigger waits for any of its events,
//! checks its conditions and then runs its effects.
//!
//! ```text
//! TRIGGERS
//!     u32 version (4 or 5), u32 next_id,
//!     u32 n, trigger[n], u32 n, (string name, f32 value)[n] variables,
//!     v4: string, u32           read and ignored by the game
//!     v5: u32 n, string[n]
//! trigger
//!     u32 version (4), u32 id, u32 unknown, u8 active, u8 repeats,
//!     f32 activation_time, string name, string comment, u32 next_element,
//!     u32 n, element[n] events, u32 n, element[n] conditions,
//!     u32 n, element[n] effects
//! element
//!     u32 version (2 or 3), string name, u32 id, u32 n, parameter[n]
//! parameter
//!     u32 version (4), string kind, string source, u8 set, value:
//!     4, 12 or 0 bytes or a string, by source (see `value_kind`)
//! ```
//!
//! Elements are instances of templates the game defines by name, such as
//! `actor_enters_area`; their parameters are typed by kind (`ma_actor`) and
//! say where the value comes from (`mi_actor_id`, `mi_actor_named`, ...).

use super::expect;
use super::header::{bool, check_version, optional};
use crate::{Cursor, Error, Result, Writer};

#[derive(Debug, Clone, PartialEq)]
pub struct Triggers {
    pub version: u32,
    /// The id the next new trigger gets.
    pub next_id: u32,
    pub triggers: Vec<Trigger>,
    /// Named values that triggers can set and compare.
    pub variables: Vec<(String, f32)>,
    /// Version 4 only.
    pub legacy: Option<(String, u32)>,
    /// From version 5 on.
    pub unknown_strings: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    /// By which `mi_trigger` parameters refer to it.
    pub id: u32,
    pub unknown: u32,
    /// Whether it starts out listening for its events.
    pub active: bool,
    /// Whether it stays active after running its effects.
    pub repeats: bool,
    /// Game time when it was last activated.
    pub activation_time: f32,
    pub name: String,
    /// The map author's note.
    pub comment: String,
    /// The id the trigger's next new element gets.
    pub next_element: u32,
    pub events: Vec<Element>,
    pub conditions: Vec<Element>,
    pub effects: Vec<Element>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub version: u32,
    /// The template's name, e.g. `actor_enters_area`.
    pub name: String,
    /// Unique within its trigger.
    pub id: u32,
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    /// What it is, e.g. `ma_actor`.
    pub kind: String,
    /// Where its value comes from, e.g. `mi_actor_named`, which decides the
    /// [`Value`]'s variant.
    pub source: String,
    /// Whether a value was given; unset parameters show a placeholder in the
    /// editor.
    pub set: bool,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    None,
    /// An integer, or the id or index of what the source names.
    Int(u32),
    Float(f32),
    Location([f32; 3]),
    /// A name or string id.
    String(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ValueKind {
    None,
    Int,
    Float,
    Location,
    String,
}

/// How the game stores a source's value. Sources it doesn't know store none.
fn value_kind(source: &str) -> ValueKind {
    use ValueKind::*;
    match source.strip_prefix("mi_").unwrap_or_default() {
        "int"
        | "compare"
        | "actor_any"
        | "actor_id"
        | "actor_condition"
        | "actor_event"
        | "kingdom_any"
        | "kingdom_all"
        | "kingdom_id"
        | "kingdom_condition"
        | "kingdom_event"
        | "hero_random"
        | "area"
        | "camera"
        | "boolean"
        | "state"
        | "trigger"
        | "actor_data_any"
        | "difficulty_level_map" => Int,
        "fixed" => Float,
        "location" => Location,
        "variable"
        | "actor_named"
        | "kingdom_named"
        | "hero_named"
        | "objective"
        | "ability"
        | "animation"
        | "technology"
        | "command"
        | "vfx"
        | "sound"
        | "speech"
        | "music"
        | "actor_data_named"
        | "organization_data"
        | "unit_data"
        | "building_data"
        | "transported_actor_data"
        | "sai_data"
        | "flare_data_location"
        | "flare_data_actor"
        | "string"
        | "localized_string"
        | "difficulty_level_specified"
        | "weather"
        | "sun"
        | "script_command"
        | "cutscene"
        | "formation"
        | "resource" => String,
        _ => None,
    }
}

impl Value {
    fn kind(&self) -> ValueKind {
        match self {
            Value::None => ValueKind::None,
            Value::Int(_) => ValueKind::Int,
            Value::Float(_) => ValueKind::Float,
            Value::Location(_) => ValueKind::Location,
            Value::String(_) => ValueKind::String,
        }
    }
}

const TRIGGER_VERSION: u32 = 4;
const PARAMETER_VERSION: u32 = 4;

impl Triggers {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let cur = &mut cur;
        let version = check_version("triggers", cur.u32()?, 4..=5)?;
        let next_id = cur.u32()?;
        let triggers = list(cur, Trigger::parse)?;
        let variables = list(cur, |cur| Ok((cur.string()?, cur.f32()?)))?;
        let triggers = Triggers {
            version,
            next_id,
            triggers,
            variables,
            legacy: (version == 4)
                .then(|| Ok::<_, Error>((cur.string()?, cur.u32()?)))
                .transpose()?,
            unknown_strings: (version >= 5)
                .then(|| list(cur, Cursor::string))
                .transpose()?,
        };
        cur.finish("TRIGGERS")?;
        Ok(triggers)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        let version = self.version;
        w.u32(version);
        w.u32(self.next_id);
        w.u32(self.triggers.len() as u32);
        self.triggers.iter().try_for_each(|t| t.write(&mut w))?;
        w.u32(self.variables.len() as u32);
        for (name, value) in &self.variables {
            w.string(name)?;
            w.f32(*value);
        }
        optional(version == 4, "legacy", self.legacy.as_ref(), |(s, v)| {
            w.string(s)?;
            w.u32(*v);
            Ok(())
        })?;
        optional(
            version >= 5,
            "unknown_strings",
            self.unknown_strings.as_ref(),
            |ss| {
                w.u32(ss.len() as u32);
                ss.iter().try_for_each(|s| w.string(s))
            },
        )?;
        Ok(w.0)
    }
}

fn list<'a, T>(
    cur: &mut Cursor<'a>,
    mut item: impl FnMut(&mut Cursor<'a>) -> Result<T>,
) -> Result<Vec<T>> {
    let n = cur.u32()?;
    (0..n).map(|_| item(cur)).collect()
}

fn write_elements(w: &mut Writer, elements: &[Element]) -> Result<()> {
    w.u32(elements.len() as u32);
    elements.iter().try_for_each(|e| e.write(w))
}

impl Trigger {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        expect(cur, "trigger version", TRIGGER_VERSION)?;
        Ok(Trigger {
            id: cur.u32()?,
            unknown: cur.u32()?,
            active: bool(cur, "trigger active")?,
            repeats: bool(cur, "trigger repeats")?,
            activation_time: cur.f32()?,
            name: cur.string()?,
            comment: cur.string()?,
            next_element: cur.u32()?,
            events: list(cur, Element::parse)?,
            conditions: list(cur, Element::parse)?,
            effects: list(cur, Element::parse)?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(TRIGGER_VERSION);
        w.u32(self.id);
        w.u32(self.unknown);
        w.u8(self.active.into());
        w.u8(self.repeats.into());
        w.f32(self.activation_time);
        w.string(&self.name)?;
        w.string(&self.comment)?;
        w.u32(self.next_element);
        write_elements(w, &self.events)?;
        write_elements(w, &self.conditions)?;
        write_elements(w, &self.effects)
    }
}

impl Element {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        Ok(Element {
            version: check_version("trigger element", cur.u32()?, 2..=3)?,
            name: cur.string()?,
            id: cur.u32()?,
            parameters: list(cur, Parameter::parse)?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(self.version);
        w.string(&self.name)?;
        w.u32(self.id);
        w.u32(self.parameters.len() as u32);
        self.parameters.iter().try_for_each(|p| p.write(w))
    }
}

impl Parameter {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        expect(cur, "trigger parameter version", PARAMETER_VERSION)?;
        let kind = cur.string()?;
        let source = cur.string()?;
        let set = bool(cur, "trigger parameter set")?;
        let value = match value_kind(&source) {
            ValueKind::None => Value::None,
            ValueKind::Int => Value::Int(cur.u32()?),
            ValueKind::Float => Value::Float(cur.f32()?),
            ValueKind::Location => Value::Location(cur.f32s()?),
            ValueKind::String => Value::String(cur.string()?),
        };
        Ok(Parameter {
            kind,
            source,
            set,
            value,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        if self.value.kind() != value_kind(&self.source) {
            return Err(Error::Unwritable(format!(
                "{:?} isn't the kind of value {} has",
                self.value, self.source
            )));
        }
        w.u32(PARAMETER_VERSION);
        w.string(&self.kind)?;
        w.string(&self.source)?;
        w.u8(self.set.into());
        match &self.value {
            Value::None => {}
            Value::Int(v) => w.u32(*v),
            Value::Float(v) => w.f32(*v),
            Value::Location(v) => w.f32s(v),
            Value::String(s) => w.string(s)?,
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameter(kind: &str, source: &str, value: Value) -> Parameter {
        Parameter {
            kind: kind.into(),
            source: source.into(),
            set: true,
            value,
        }
    }

    fn triggers(version: u32) -> Triggers {
        Triggers {
            version,
            next_id: 3,
            triggers: vec![Trigger {
                id: 1,
                unknown: 1,
                active: true,
                repeats: false,
                activation_time: 0.0,
                name: "A 00.00 Map Init".into(),
                comment: String::new(),
                next_element: 4,
                events: vec![Element {
                    version: 3,
                    name: "actor_enters_area".into(),
                    id: 1,
                    parameters: vec![
                        parameter("ma_actor", "mi_actor_id", Value::Int(68924)),
                        parameter("ma_area", "mi_area", Value::Int(461)),
                    ],
                }],
                conditions: vec![Element {
                    version: 2,
                    name: "value_compare_value".into(),
                    id: 2,
                    parameters: vec![
                        parameter("ma_value", "mi_fixed", Value::Float(0.5)),
                        parameter("ma_value", "mi_range_int", Value::None),
                    ],
                }],
                effects: vec![Element {
                    version: 3,
                    name: "actor_moves_to_location".into(),
                    id: 3,
                    parameters: vec![
                        parameter("ma_actor", "mi_actor_named", Value::String("tank".into())),
                        parameter(
                            "ma_location",
                            "mi_location",
                            Value::Location([1.0, 2.0, 0.0]),
                        ),
                    ],
                }],
            }],
            variables: vec![("count".into(), 2.0)],
            legacy: (version == 4).then(|| (String::new(), 0)),
            unknown_strings: (version >= 5).then(|| vec!["x".into()]),
        }
    }

    #[test]
    fn round_trips_each_version() {
        for version in [4, 5] {
            let triggers = triggers(version);
            let bytes = triggers.to_bytes().unwrap();
            assert_eq!(Triggers::parse(&bytes).unwrap(), triggers);
        }
    }

    #[test]
    fn lays_out_parameters() {
        let mut w = Writer::default();
        parameter(
            "ma_location",
            "mi_location",
            Value::Location([1.0, 2.0, 3.0]),
        )
        .write(&mut w)
        .unwrap();
        let mut want = Writer::default();
        want.u32(4);
        want.string("ma_location").unwrap();
        want.string("mi_location").unwrap();
        want.u8(1);
        want.f32s(&[1.0, 2.0, 3.0]);
        assert_eq!(w.0, want.0);
    }

    #[test]
    fn refuses_mismatched_values_and_fields() {
        let mut t = triggers(5);
        t.triggers[0].events[0].parameters[0].value = Value::String("1".into());
        assert!(t.to_bytes().is_err());
        let mut t = triggers(5);
        t.legacy = Some((String::new(), 0));
        assert!(t.to_bytes().is_err());
        let mut bytes = triggers(4).to_bytes().unwrap();
        bytes[12] = 3;
        assert!(Triggers::parse(&bytes).is_err());
    }
}
