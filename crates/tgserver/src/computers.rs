//! The computer players a match's host runs, each playing by its side's
//! profile.

use std::collections::BTreeMap;
use std::sync::Arc;

use tgdata::sai::Profile;
use tgdata::{GameData, Id, Sai};
use tgsim::{ComputerPlayer, Planner, World};

use crate::Setup;

/// The profiles a match's computer players play by.
#[derive(Debug, Clone, Default)]
pub struct Profiles {
    /// Each side's chosen at launch, by player id.
    chosen: BTreeMap<u32, Arc<Profile>>,
    /// What those go by, by player id.
    names: BTreeMap<u32, String>,
    /// Those a side with none chosen may be given, one at random.
    others: Vec<Arc<Profile>>,
    /// What a computer player taking each side over mid-match plays by, by
    /// player id, with the name the profile goes by.
    taking_over: BTreeMap<u32, (Arc<Profile>, String)>,
}

impl Profiles {
    /// Reads the profiles of `chosen` and `taking_over`, by player id, and
    /// the public ones, out of `sais`.
    pub fn new(
        game: &GameData,
        chosen_ids: &BTreeMap<u32, Id>,
        taking_over: &BTreeMap<u32, Id>,
        sais: &[Sai],
    ) -> Profiles {
        let public = sais
            .iter()
            .filter(|s| s.public && !s.easiest)
            .map(|s| &s.id);
        let ids: Vec<&Id> = (chosen_ids.values().chain(taking_over.values()))
            .chain(public)
            .collect();
        let read: Vec<Arc<Profile>> = game
            .profiles(ids.iter().copied())
            .into_iter()
            .map(Arc::new)
            .collect();
        let find = |id: &Id| read.iter().find(|p| p.id == *id).cloned();
        let chosen = (chosen_ids.iter())
            .filter_map(|(&p, id)| Some((p, find(id)?)))
            .collect();
        let others = (sais.iter())
            .filter(|s| s.public && !s.easiest)
            .filter_map(|s| find(&s.id))
            .collect();
        let named = |id: &Id| sais.iter().find(|s| s.id == *id).map(|s| s.name.clone());
        let names = (chosen_ids.iter())
            .filter_map(|(&p, id)| Some((p, named(id)?)))
            .collect();
        let taking_over = (taking_over.iter())
            .filter_map(|(&p, id)| Some((p, (find(id)?, named(id)?))))
            .collect();
        Profiles {
            chosen,
            names,
            others,
            taking_over,
        }
    }

    /// Profiles of their own, for tests.
    #[cfg(test)]
    fn of(chosen: BTreeMap<u32, Arc<Profile>>, others: Vec<Arc<Profile>>) -> Profiles {
        Profiles {
            chosen,
            names: BTreeMap::new(),
            others,
            taking_over: BTreeMap::new(),
        }
    }

    /// The computer player for side `player` of the match `setup` set up:
    /// one by the profile chosen for it, else one at random, else one that
    /// gives no orders.
    pub fn player(&self, setup: &Setup, _: &World, player: u32) -> Box<dyn ComputerPlayer> {
        let seed = seed(setup, player);
        let Some(profile) = self.profile(seed, player) else {
            return Box::new(tgsim::Idle);
        };
        Box::new(Planner::new(profile, seed, self.offset(player)))
    }

    /// What the profile chosen at launch for each side with one goes by,
    /// by player id.
    pub fn names(&self) -> &BTreeMap<u32, String> {
        &self.names
    }

    /// The computer player that takes side `player` of the match `setup`
    /// set up over mid-match, with a fresh plan, and the name of the
    /// profile it plays by; one that gives no orders, and no name, where
    /// the side has none.
    pub fn taker(&self, setup: &Setup, player: u32) -> (Box<dyn ComputerPlayer>, Option<String>) {
        match self.taking_over.get(&player) {
            Some((profile, name)) => {
                let seed = seed(setup, player);
                let planner = Planner::new(profile.clone(), seed, self.offset(player));
                (Box::new(planner), Some(name.clone()))
            }
            None => (Box::new(tgsim::Idle), None),
        }
    }

    /// How far into a period side `player`'s computer player plans its
    /// second time early: the i-th of n by i / (n + 1) of one.
    fn offset(&self, player: u32) -> f32 {
        let n = self.chosen.len().max(1);
        let i = (self.chosen.keys().position(|&p| p == player)).unwrap_or(n - 1) + 1;
        i as f32 / (n + 1) as f32
    }

    /// The profile side `player` plays by, drawn by `seed` where none was
    /// chosen for it.
    fn profile(&self, seed: u64, player: u32) -> Option<Arc<Profile>> {
        self.chosen.get(&player).cloned().or_else(|| {
            let n = self.others.len();
            (n > 0).then(|| self.others[(seed % n as u64) as usize].clone())
        })
    }
}

/// What side `player`'s computer player draws its chances from.
fn seed(setup: &Setup, player: u32) -> u64 {
    setup.seed ^ u64::from(player).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str) -> Arc<Profile> {
        Arc::new(Profile {
            id: Id::from(id),
            ego_fuzziness: 0.0,
            max_ge_commands: 100,
            max_rai_commands: 40,
            moods: Vec::new(),
        })
    }

    #[test]
    fn a_side_plays_by_its_chosen_profile_else_one_at_random() {
        let profiles = Profiles::of(
            [(2, profile("konev_sai"))].into(),
            vec![profile("patton_sai"), profile("rommel_sai")],
        );
        let id = |p: Option<Arc<Profile>>| p.map(|p| p.id.to_string());
        assert_eq!(id(profiles.profile(7, 2)), Some("konev_sai".into()));
        assert_eq!(id(profiles.profile(7, 3)), Some("rommel_sai".into()));
        assert_eq!(id(profiles.profile(8, 3)), Some("patton_sai".into()));
        assert_eq!(id(Profiles::default().profile(7, 3)), None);
    }

    #[test]
    fn a_side_taken_over_plays_by_its_own_profile_not_the_one_chosen() {
        let mut profiles = Profiles::of([(2, profile("konev_sai"))].into(), Vec::new());
        let taking_over = (profile("zhukov_sai"), "Zhukov AI".to_string());
        profiles.taking_over.insert(2, taking_over);
        let setup = Setup::default();
        assert_eq!(profiles.taker(&setup, 2).1.as_deref(), Some("Zhukov AI"));
        assert_eq!(profiles.taker(&setup, 3).1, None);
    }
}
