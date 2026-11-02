//! OBJECTIVES: the goals a map sets its kingdoms, which triggers then give,
//! complete or fail.
//!
//! ```text
//! OBJECTIVES
//!     u32 count, then per objective:
//!       u32 version (0), string name, string text, u8 primary
//!     u32 next_number
//! ```

use super::expect;
use super::header::bool;
use crate::{Cursor, Result, Writer};

#[derive(Debug, Clone, PartialEq)]
pub struct Objectives {
    pub objectives: Vec<Objective>,
    /// Above every objective's number in shipped maps, even where higher ones
    /// were deleted, so presumably what the editor numbers the next new one.
    pub next_number: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Objective {
    /// E.g. `objective_0`, by which triggers and kingdoms refer to it.
    pub name: String,
    /// A string id; the same as the name in every shipped map.
    pub text: String,
    /// Shown with the primary objectives rather than the optional ones.
    pub primary: bool,
}

impl Objectives {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let cur = &mut cur;
        let count = cur.u32()?;
        let objectives = (0..count)
            .map(|_| {
                expect(cur, "objective version", 0)?;
                Ok(Objective {
                    name: cur.string()?,
                    text: cur.string()?,
                    primary: bool(cur, "objective primary")?,
                })
            })
            .collect::<Result<_>>()?;
        let next_number = cur.u32()?;
        cur.finish("OBJECTIVES")?;
        Ok(Objectives {
            objectives,
            next_number,
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        w.u32(self.objectives.len() as u32);
        for objective in &self.objectives {
            w.u32(0);
            w.string(&objective.name)?;
            w.string(&objective.text)?;
            w.u8(objective.primary.into());
        }
        w.u32(self.next_number);
        Ok(w.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn objective(n: u32, primary: bool) -> Objective {
        let name = format!("objective_{n}");
        Objective {
            text: name.clone(),
            name,
            primary,
        }
    }

    #[test]
    fn round_trips() {
        for objectives in [
            Objectives {
                objectives: Vec::new(),
                next_number: 0,
            },
            Objectives {
                objectives: vec![objective(0, true), objective(3, false)],
                next_number: 5,
            },
        ] {
            let bytes = objectives.to_bytes().unwrap();
            assert_eq!(Objectives::parse(&bytes).unwrap(), objectives);
        }
    }

    #[test]
    fn lays_out_objectives() {
        let objectives = Objectives {
            objectives: vec![objective(2, false)],
            next_number: 3,
        };
        let mut want = Writer::default();
        want.u32(1);
        want.u32(0);
        want.string("objective_2").unwrap();
        want.string("objective_2").unwrap();
        want.u8(0);
        want.u32(3);
        assert_eq!(objectives.to_bytes().unwrap(), want.0);
    }

    #[test]
    fn refuses_unknown_versions_and_trailing_bytes() {
        let mut bytes = Objectives {
            objectives: vec![objective(0, true)],
            next_number: 1,
        }
        .to_bytes()
        .unwrap();
        bytes.push(0);
        assert!(Objectives::parse(&bytes).is_err());
        bytes.pop();
        bytes[4] = 1;
        assert!(Objectives::parse(&bytes).is_err());
    }
}
