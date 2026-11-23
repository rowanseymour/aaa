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
    /// Those a side taken over in the match may be given, one at random.
    others: Vec<Arc<Profile>>,
}

impl Profiles {
    /// Reads the profiles of `chosen`, by player id, and the public ones,
    /// out of `sais`, for sides taken over later.
    pub fn new(game: &GameData, chosen: &BTreeMap<u32, Id>, sais: &[Sai]) -> Profiles {
        let public = sais
            .iter()
            .filter(|s| s.public && !s.easiest)
            .map(|s| &s.id);
        let ids: Vec<&Id> = chosen.values().chain(public).collect();
        let read: Vec<Arc<Profile>> = game
            .profiles(ids.iter().copied())
            .into_iter()
            .map(Arc::new)
            .collect();
        let find = |id: &Id| read.iter().find(|p| p.id == *id).cloned();
        let chosen = (chosen.iter())
            .filter_map(|(&p, id)| Some((p, find(id)?)))
            .collect();
        let others = (sais.iter())
            .filter(|s| s.public && !s.easiest)
            .filter_map(|s| find(&s.id))
            .collect();
        Profiles { chosen, others }
    }

    /// Profiles of their own, for tests.
    #[cfg(test)]
    fn of(chosen: BTreeMap<u32, Arc<Profile>>, others: Vec<Arc<Profile>>) -> Profiles {
        Profiles { chosen, others }
    }

    /// The computer player for side `player` of the match `setup` set up:
    /// one by the profile chosen for it, else one at random, else one that
    /// gives no orders.
    pub fn player(&self, setup: &Setup, _: &World, player: u32) -> Box<dyn ComputerPlayer> {
        let seed = seed(setup, player);
        let Some(profile) = self.profile(seed, player) else {
            return Box::new(tgsim::Idle);
        };
        // The i-th of n plans its second time i / (n + 1) of a period early.
        let n = self.chosen.len().max(1);
        let i = self
            .chosen
            .keys()
            .position(|&p| p == player)
            .unwrap_or(n - 1)
            + 1;
        Box::new(Planner::new(profile, seed, i as f32 / (n + 1) as f32))
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
}
