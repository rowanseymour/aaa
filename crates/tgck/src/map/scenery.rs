//! The detail of reactive scenery, laid out as `reactive_scenery` in
//! [`rmp.ksy`].
//!
//! [`rmp.ksy`]: https://github.com/rowanseymour/aaa/blob/main/docs/formats/rmp.ksy

use super::actor::Updatable;
use super::expect;
use super::header::{self, check_version};
use crate::{Cursor, Result, Writer};

/// Trees, grass, hedges and the like, with up to two parts that react to
/// units and blasts.
#[derive(Debug, Clone, PartialEq)]
pub struct ReactiveScenery {
    /// The parts its template had when saved, a bit each as in `parts`;
    /// the game adds any the template has gained since.
    pub template_parts: u32,
    /// Set off by units passing through, such as tall grass.
    pub stir: Option<Updatable>,
    pub topple: Option<Topple>,
}

/// The part that falls when hit hard enough, and may grow back.
#[derive(Debug, Clone, PartialEq)]
pub struct Topple {
    pub version: u32,
    pub updatable: Updatable,
    pub state: ToppleState,
    /// The game time the state began.
    pub state_time: f32,
    /// The game time it began growing back.
    pub regrow_time: f32,
    /// The way it falls, a unit vector in its own frame.
    pub direction: [f32; 2],
    /// From version 1 on: how far grown it was when it fell, 0 to 1.
    pub growth: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToppleState {
    Standing,
    Trampled,
    Falling,
    Down,
    Bouncing,
    Dead,
    Sinking,
    Regrowing,
    /// Not one the game sets.
    Other(u32),
}

const STIR: u32 = 1;
const TOPPLE: u32 = 2;

impl ReactiveScenery {
    pub(super) fn parse(cur: &mut Cursor) -> Result<Self> {
        expect(cur, "reactive scenery version", 0)?;
        let parts = cur.u32()?;
        if parts & !(STIR | TOPPLE) != 0 {
            return Err(crate::Error::Corrupt(format!("scenery parts {parts:#x}")));
        }
        let template_parts = cur.u32()?;
        let stir = (parts & STIR != 0).then(|| part(cur)).transpose()?;
        let topple = (parts & TOPPLE != 0)
            .then(|| Topple::parse(cur))
            .transpose()?;
        Ok(ReactiveScenery {
            template_parts,
            stir,
            topple,
        })
    }

    pub(super) fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(0);
        let parts =
            (u32::from(self.stir.is_some()) * STIR) | (u32::from(self.topple.is_some()) * TOPPLE);
        w.u32(parts);
        w.u32(self.template_parts);
        if let Some(u) = &self.stir {
            write_part(w, u);
        }
        match &self.topple {
            Some(t) => t.write(w),
            None => Ok(()),
        }
    }
}

/// What each part holds: its id among the world's updatables.
fn part(cur: &mut Cursor) -> Result<Updatable> {
    expect(cur, "scenery part version", 0)?;
    let updatable = Updatable::parse(cur)?;
    expect(cur, "scenery part base version", 0)?;
    Ok(updatable)
}

fn write_part(w: &mut Writer, updatable: &Updatable) {
    w.u32(0);
    updatable.write(w);
    w.u32(0);
}

impl Topple {
    fn parse(cur: &mut Cursor) -> Result<Self> {
        let version = check_version("topple", cur.u32()?, 0..=1)?;
        Ok(Topple {
            version,
            updatable: part(cur)?,
            state: ToppleState::from(cur.u32()?),
            state_time: cur.f32()?,
            regrow_time: cur.f32()?,
            direction: cur.f32s()?,
            growth: (version >= 1).then(|| cur.f32()).transpose()?,
        })
    }

    fn write(&self, w: &mut Writer) -> Result<()> {
        w.u32(self.version);
        write_part(w, &self.updatable);
        w.u32(self.state.into());
        w.f32(self.state_time);
        w.f32(self.regrow_time);
        w.f32s(&self.direction);
        header::optional(self.version >= 1, "topple growth", self.growth, |g| {
            w.f32(g);
            Ok(())
        })
    }
}

const TOPPLE_STATES: [ToppleState; 8] = [
    ToppleState::Standing,
    ToppleState::Trampled,
    ToppleState::Falling,
    ToppleState::Down,
    ToppleState::Bouncing,
    ToppleState::Dead,
    ToppleState::Sinking,
    ToppleState::Regrowing,
];

impl From<u32> for ToppleState {
    fn from(v: u32) -> Self {
        TOPPLE_STATES
            .get(v as usize)
            .copied()
            .unwrap_or(ToppleState::Other(v))
    }
}

impl From<ToppleState> for u32 {
    fn from(s: ToppleState) -> Self {
        match s {
            ToppleState::Other(v) => v,
            s => TOPPLE_STATES.iter().position(|&t| t == s).unwrap() as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(s: &ReactiveScenery) -> Vec<u8> {
        let mut w = Writer::default();
        s.write(&mut w).unwrap();
        let mut cur = Cursor::new(&w.0);
        assert_eq!(&ReactiveScenery::parse(&mut cur).unwrap(), s);
        cur.finish("scenery").unwrap();
        w.0
    }

    #[test]
    fn round_trips_reactive_scenery() {
        let updatable = |id| Updatable { version: 1, id };
        let both = ReactiveScenery {
            template_parts: 3,
            stir: Some(updatable(-1)),
            topple: Some(Topple {
                version: 1,
                updatable: updatable(40),
                state: ToppleState::Down,
                state_time: 12.5,
                regrow_time: -1.0,
                direction: [0.0, 1.0],
                growth: Some(0.5),
            }),
        };
        let bytes = round_trip(&both);
        assert_eq!(bytes[4..12], [3, 0, 0, 0, 3, 0, 0, 0], "parts bits");

        let none = ReactiveScenery {
            template_parts: 2,
            stir: None,
            topple: None,
        };
        assert_eq!(round_trip(&none).len(), 12);
    }

    #[test]
    fn keeps_unknown_topple_states() {
        assert_eq!(u32::from(ToppleState::from(9)), 9);
        assert_eq!(u32::from(ToppleState::from(7)), 7);
        assert_eq!(ToppleState::from(7), ToppleState::Regrowing);
    }

    #[test]
    fn rejects_unknown_parts() {
        let mut w = Writer::default();
        [0, 4, 4].iter().for_each(|&v| w.u32(v));
        assert!(ReactiveScenery::parse(&mut Cursor::new(&w.0)).is_err());
    }
}
