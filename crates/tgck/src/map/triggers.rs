//! TRIGGERS, the map's scripting, laid out as [`rmp.ksy`] specifies.
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use crate::{Cursor, Error, Result, Writer};

/// The TRIGGERS section, the `triggers` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Triggers {
    pub version: u32,
    pub next_id: u32,
    pub triggers: Vec<Trigger>,
    pub variables: Vec<(String, f32)>,
    /// `legacy_string`, then `legacy_value`.
    pub legacy: Option<(String, u32)>,
    pub unused_strings: Option<Vec<String>>,
}

/// A `trigger` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    pub id: u32,
    pub all_conditions: bool,
    pub active: bool,
    pub repeats: bool,
    pub activation_time: f32,
    pub name: String,
    pub comment: String,
    pub next_element: u32,
    pub events: Vec<Element>,
    pub conditions: Vec<Element>,
    pub effects: Vec<Element>,
}

/// A `trigger_element` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub version: u32,
    pub name: String,
    pub id: u32,
    pub parameters: Vec<Parameter>,
}

/// A `parameter` of `rmp.ksy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub kind: String,
    /// Decides the [`Value`]'s variant.
    pub source: String,
    pub set: bool,
    pub value: Value,
}

/// A [`Parameter`]'s value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// For a source the game doesn't know.
    None,
    Int(u32),
    Float(f32),
    Location([f32; 3]),
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
        let version = cur.version("triggers", 4..=5)?;
        let next_id = cur.u32()?;
        let triggers = cur.list(Trigger::parse)?;
        let variables = cur.pairs(Cursor::f32)?;
        let triggers = Triggers {
            version,
            next_id,
            triggers,
            variables,
            legacy: (version == 4)
                .then(|| Ok::<_, Error>((cur.string()?, cur.u32()?)))
                .transpose()?,
            unused_strings: (version >= 5).then(|| cur.strings()).transpose()?,
        };
        cur.finish("TRIGGERS")?;
        Ok(triggers)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        let version = self.version;
        w.u32(version);
        w.u32(self.next_id);
        w.list("triggers", &self.triggers, |w, t| t.write(w))?;
        w.pairs("variables", &self.variables, Writer::f32)?;
        w.optional(version == 4, "legacy", self.legacy.as_ref(), |w, (s, v)| {
            w.string(s)?;
            w.u32(*v);
            Ok(())
        })?;
        w.optional(
            version >= 5,
            "unused_strings",
            self.unused_strings.as_deref(),
            |w, v| w.strings("unused strings", v),
        )?;
        Ok(w.0)
    }
}

fn write_elements(w: &mut Writer, elements: &[Element]) -> Result<()> {
    w.list("elements", elements, |w, e| e.write(w))
}

impl Trigger {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        cur.expect("trigger version", TRIGGER_VERSION)?;
        Ok(Trigger {
            id: cur.u32()?,
            all_conditions: match cur.u32()? {
                0 => false,
                1 => true,
                v => return Err(Error::Corrupt(format!("trigger condition mode is {v}"))),
            },
            active: cur.bool("trigger active")?,
            repeats: cur.bool("trigger repeats")?,
            activation_time: cur.f32()?,
            name: cur.string()?,
            comment: cur.string()?,
            next_element: cur.u32()?,
            events: cur.list(Element::parse)?,
            conditions: cur.list(Element::parse)?,
            effects: cur.list(Element::parse)?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(TRIGGER_VERSION);
        w.u32(self.id);
        w.u32(self.all_conditions.into());
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
            version: cur.version("trigger element", 2..=3)?,
            name: cur.string()?,
            id: cur.u32()?,
            parameters: cur.list(Parameter::parse)?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(self.version);
        w.string(&self.name)?;
        w.u32(self.id);
        w.list("parameters", &self.parameters, |w, p| p.write(w))
    }
}

impl Parameter {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        cur.expect("trigger parameter version", PARAMETER_VERSION)?;
        let kind = cur.string()?;
        let source = cur.string()?;
        let set = cur.bool("trigger parameter set")?;
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
pub(super) mod tests {
    use super::*;

    fn parameter(kind: &str, source: &str, value: Value) -> Parameter {
        Parameter {
            kind: kind.into(),
            source: source.into(),
            set: true,
            value,
        }
    }

    pub(in crate::map) fn triggers(version: u32) -> Triggers {
        Triggers {
            version,
            next_id: 3,
            triggers: vec![Trigger {
                id: 1,
                all_conditions: true,
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
            unused_strings: (version >= 5).then(|| vec!["x".into()]),
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
