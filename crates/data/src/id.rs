use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use serde::Deserialize;

/// An id the game matches ignoring case, such as a template's or terrain's
/// `IDS`: equal, hashed and ordered by its ASCII-lower-case form, but kept
/// as written.
#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub struct Id(Box<str>);

impl Id {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn folded(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.bytes().map(|b| b.to_ascii_lowercase())
    }
}

impl From<&str> for Id {
    fn from(s: &str) -> Self {
        Id(s.into())
    }
}

impl From<String> for Id {
    fn from(s: String) -> Self {
        Id(s.into())
    }
}

impl PartialEq for Id {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for Id {}

impl Hash for Id {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for b in self.folded() {
            state.write_u8(b);
        }
        // As `str` does, so that ids hashed in sequence stay distinct.
        state.write_u8(0xff);
    }
}

impl Ord for Id {
    fn cmp(&self, other: &Self) -> Ordering {
        self.folded().cmp(other.folded())
    }
}

impl PartialOrd for Id {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use super::*;

    #[test]
    fn ignores_case() {
        assert_eq!(Id::from("Temperate_Grass01"), Id::from("temperate_grass01"));
        assert_ne!(Id::from("grass01"), Id::from("grass02"));
        let map: HashMap<Id, i32> = [(Id::from("USA_Tank"), 1)].into();
        assert_eq!(map.get(&Id::from("usa_tank")), Some(&1));
        let sorted: Vec<String> = BTreeSet::from(["b", "A", "a_", "C"].map(Id::from))
            .iter()
            .map(Id::to_string)
            .collect();
        assert_eq!(sorted, ["A", "a_", "b", "C"]);
    }
}
