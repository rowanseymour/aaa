//! Films: matches kept as what they were set up from and given, so that
//! one can be played again.
//!
//! A film's file is [`MAGIC`], then the version of the layout as a
//! little-endian `u16`, then the [`Film`] as `postcard` encodes it.

use std::fmt;
use std::path::Path;

use aaa_sim::Record;
use serde::{Deserialize, Serialize};

use crate::Setup;

/// What a film's file starts with.
const MAGIC: &[u8; 4] = b"film";

/// The layout this build writes, and the only one it reads: a film's steps
/// only play out the same under the rules it was recorded with, so there's
/// nothing to gain from reading an older one.
const VERSION: u16 = 1;

/// A match as it was played.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Film {
    pub setup: Setup,
    pub record: Record,
}

/// Why bytes aren't a film this build can play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotFilm {
    /// They don't start as a film does.
    Unrecognised,
    /// They're a film of another layout: this version.
    Version(u16),
    /// They start as a film does and then aren't one.
    Damaged(String),
}

impl fmt::Display for NotFilm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NotFilm::Unrecognised => write!(f, "not a film"),
            NotFilm::Version(v) => write!(f, "a film of version {v}, not {VERSION}"),
            NotFilm::Damaged(e) => write!(f, "a damaged film: {e}"),
        }
    }
}

impl Film {
    /// The film as its file holds it.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend(VERSION.to_le_bytes());
        // Writing to memory, which nothing a film holds can fail at.
        postcard::to_extend(self, out).expect("a film encodes")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Film, NotFilm> {
        let rest = bytes.strip_prefix(MAGIC).ok_or(NotFilm::Unrecognised)?;
        let (version, rest) = rest.split_first_chunk().ok_or(NotFilm::Unrecognised)?;
        match u16::from_le_bytes(*version) {
            VERSION => match postcard::take_from_bytes(rest) {
                Ok((film, [])) => Ok(film),
                Ok(_) => Err(NotFilm::Damaged("more follows where it ends".into())),
                Err(e) => Err(NotFilm::Damaged(e.to_string())),
            },
            other => Err(NotFilm::Version(other)),
        }
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, self.to_bytes()).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn read(path: &Path) -> Result<Film, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Film::from_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use aaa_sim::{Action, Command, Given, Piece};

    use super::*;

    fn film() -> Film {
        let given = |step, action| Given { step, action };
        let order = |command| Action::Order { player: 2, command };
        let raise = Action::Raise {
            template: "Truck".into(),
            owner: 3,
            at: [1.5, -2.0],
            facing: [0.0, 1.0],
        };
        let guard = Command::GuardPiece {
            organization: 4,
            piece: Piece::Building(1),
        };
        let rally = Command::Rally {
            building: 0,
            at: None,
        };
        Film {
            setup: Setup {
                map: "berlin".into(),
                seed: u64::MAX,
                teams: Some(vec![vec![1, 2], vec![3]]),
                nations: [(1, "nation_usa".into())].into(),
                fog: Some(tgmap::FogSetting::Explored),
                ..Default::default()
            },
            record: Record {
                steps: 600,
                given: vec![
                    given(0, Action::Speed(2.0)),
                    given(3, order(guard)),
                    given(3, order(rally)),
                    given(40, raise),
                    given(599, Action::Pause(true)),
                ],
                checks: vec![(10, 7), (20, u64::MAX)],
            },
        }
    }

    #[test]
    fn a_film_reads_back_as_it_was_written() {
        let film = film();
        let bytes = film.to_bytes();
        assert_eq!(Film::from_bytes(&bytes), Ok(film.clone()));
        // And is the same bytes every time.
        assert_eq!(film.to_bytes(), bytes);
    }

    #[test]
    fn what_is_not_a_film_this_build_plays_is_refused() {
        let bytes = film().to_bytes();
        assert_eq!(Film::from_bytes(b""), Err(NotFilm::Unrecognised));
        assert_eq!(Film::from_bytes(b"film"), Err(NotFilm::Unrecognised));
        assert_eq!(Film::from_bytes(b"map\0\x01\0"), Err(NotFilm::Unrecognised));
        let mut newer = bytes.clone();
        newer[4] = 9;
        assert_eq!(Film::from_bytes(&newer), Err(NotFilm::Version(9)));
        let cut = &bytes[..bytes.len() - 3];
        assert!(matches!(Film::from_bytes(cut), Err(NotFilm::Damaged(_))));
        let longer = [&bytes[..], b"film"].concat();
        assert!(matches!(
            Film::from_bytes(&longer),
            Err(NotFilm::Damaged(_))
        ));
    }
}
