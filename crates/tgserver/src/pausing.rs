//! Who may pause a match played between machines, and unpause it: whoever
//! paused it at once, anyone else by vote.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use tgsim::{Action, Pausing, Votes};

/// How long after a pause of theirs is voted off a person can't pause, and
/// how long after they unpause their own pause the votes given during it
/// stand should they pause again.
pub(crate) const RECENT: Duration = Duration::from_secs(5 * 60);

/// A person in the match, as pausing has them: their client, and the
/// first side they play, if any.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Person {
    pub client: u32,
    pub side: Option<u32>,
}

/// How pausing the match stands.
#[derive(Debug, Default)]
pub(crate) struct Pauses {
    /// The client that paused the match, while it's paused.
    pauser: Option<u32>,
    /// The clients for unpausing it.
    votes: BTreeSet<u32>,
    /// When each client whose pause was voted off last had it voted off.
    voted_off: HashMap<u32, Instant>,
    /// The client that last unpaused a pause of its own, and when.
    unpaused: Option<(u32, Instant)>,
}

impl Pauses {
    /// What every copy of the match is given for `client` asking at `now`
    /// to pause the match, or to unpause it, among `people`, everyone in
    /// the match. Nothing for asking what can't be done, or for what the
    /// match already is.
    pub fn ask(
        &mut self,
        client: u32,
        pause: bool,
        now: Instant,
        people: &[Person],
    ) -> Vec<Action> {
        let side = |client| people.iter().find(|p| p.client == client)?.side;
        // Only a person with a side pauses or unpauses.
        let Some(by) = side(client) else {
            return Vec::new();
        };
        let tally = |votes: &BTreeSet<u32>| {
            let sided = people.iter().filter(|p| p.side.is_some()).count();
            let votes = votes.iter().filter(|&&c| side(c).is_some()).count() as u32;
            let needed = crate::majority(sided);
            Votes { votes, needed }
        };
        // Three or more people, observers among them, take a vote.
        let voting = people.len() >= 3;
        let recent = |at: Instant| now < at + RECENT;
        match (pause, self.pauser) {
            (true, Some(_)) | (false, None) => Vec::new(),
            (true, None) => {
                if self.voted_off.get(&client).is_some_and(|&at| recent(at)) {
                    return vec![Action::Pausing(Pausing::Refused { by })];
                }
                // Pausing again soon after unpausing their own keeps the
                // votes given during it.
                let again = matches!(self.unpaused, Some((c, at)) if c == client && recent(at));
                if !again {
                    self.votes.clear();
                }
                self.pauser = Some(client);
                let mut given = vec![Action::Pause(true), Action::Pausing(Pausing::Paused { by })];
                if voting {
                    given.push(Action::Pausing(Pausing::Needed(tally(&self.votes))));
                }
                given
            }
            (false, Some(pauser)) if pauser == client => {
                self.pauser = None;
                self.unpaused = Some((client, now));
                vec![
                    Action::Pause(false),
                    Action::Pausing(Pausing::Unpaused { by }),
                ]
            }
            (false, Some(_)) if !voting => Vec::new(),
            (false, Some(pauser)) => {
                let cast = self.votes.insert(client);
                if !cast {
                    self.votes.remove(&client);
                }
                let votes = tally(&self.votes);
                let mut given = vec![Action::Pausing(Pausing::Voted { by, cast, votes })];
                if votes.votes >= votes.needed {
                    self.voted_off.insert(pauser, now);
                    self.votes.clear();
                    (self.pauser, self.unpaused) = (None, None);
                    given.extend([Action::Pause(false), Action::Pausing(Pausing::VotedOff)]);
                }
                given
            }
        }
    }

    /// What every copy of the match is given for `client` leaving it: its
    /// vote goes, and a pause of its is let go, saying nothing.
    pub fn leave(&mut self, client: u32) -> Vec<Action> {
        self.votes.remove(&client);
        match self.pauser == Some(client) {
            true => {
                self.pauser = None;
                vec![Action::Pause(false)]
            }
            false => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clients 1 to `sided` each playing side 10 more than their own id,
    /// and as many more as `watching` with no side.
    fn people(sided: u32, watching: u32) -> Vec<Person> {
        let side = |client| (client <= sided).then_some(client + 10);
        let people = (1..=sided + watching).map(|client| Person {
            client,
            side: side(client),
        });
        people.collect()
    }

    fn tally(votes: u32, needed: u32) -> Votes {
        Votes { votes, needed }
    }

    #[test]
    fn whoever_paused_unpauses_at_once_and_alone_with_two() {
        let (mut p, now, two) = (Pauses::default(), Instant::now(), people(2, 0));
        let paused = [
            Action::Pause(true),
            Action::Pausing(Pausing::Paused { by: 11 }),
        ];
        assert_eq!(p.ask(1, true, now, &two), paused);
        // Paused already, or not theirs to unpause.
        assert_eq!(p.ask(2, true, now, &two), []);
        assert_eq!(p.ask(2, false, now, &two), []);
        let unpaused = [
            Action::Pause(false),
            Action::Pausing(Pausing::Unpaused { by: 11 }),
        ];
        assert_eq!(p.ask(1, false, now, &two), unpaused);
        assert_eq!(p.ask(1, false, now, &two), []);
    }

    #[test]
    fn one_watching_neither_pauses_nor_votes_but_makes_a_vote_be_taken() {
        let (mut p, now, three) = (Pauses::default(), Instant::now(), people(2, 1));
        assert_eq!(p.ask(3, true, now, &three), []);
        let needed = Action::Pausing(Pausing::Needed(tally(0, 2)));
        assert_eq!(p.ask(1, true, now, &three)[2], needed);
        assert_eq!(p.ask(3, false, now, &three), []);
        // The other's vote alone isn't enough of two with sides.
        let voted = Pausing::Voted {
            by: 12,
            cast: true,
            votes: tally(1, 2),
        };
        assert_eq!(p.ask(2, false, now, &three), [Action::Pausing(voted)]);
    }

    #[test]
    fn enough_votes_unpause_and_keep_the_pauser_from_pausing_for_a_while() {
        let (mut p, now, four) = (Pauses::default(), Instant::now(), people(4, 0));
        p.ask(1, true, now, &four);
        let voted = |by, cast, votes| {
            let votes = tally(votes, 3);
            Action::Pausing(Pausing::Voted { by, cast, votes })
        };
        assert_eq!(p.ask(2, false, now, &four), [voted(12, true, 1)]);
        // Asking again takes the vote back.
        assert_eq!(p.ask(2, false, now, &four), [voted(12, false, 0)]);
        p.ask(2, false, now, &four);
        p.ask(3, false, now, &four);
        let off = [
            voted(14, true, 3),
            Action::Pause(false),
            Action::Pausing(Pausing::VotedOff),
        ];
        assert_eq!(p.ask(4, false, now, &four), off);
        let soon = now + RECENT - Duration::from_secs(1);
        let refused = [Action::Pausing(Pausing::Refused { by: 11 })];
        assert_eq!(p.ask(1, true, soon, &four), refused);
        // Anyone else may, and the votes start again.
        let paused = p.ask(2, true, soon, &four);
        assert_eq!(paused[1], Action::Pausing(Pausing::Paused { by: 12 }));
        assert_eq!(paused[2], Action::Pausing(Pausing::Needed(tally(0, 3))));
        p.ask(2, false, soon, &four);
        assert_eq!(p.ask(1, true, now + RECENT, &four).len(), 3);
    }

    #[test]
    fn pausing_again_soon_after_unpausing_their_own_keeps_the_votes() {
        let (mut p, now, three) = (Pauses::default(), Instant::now(), people(3, 0));
        p.ask(1, true, now, &three);
        p.ask(2, false, now, &three);
        p.ask(1, false, now, &three);
        let needed = |votes| Action::Pausing(Pausing::Needed(tally(votes, 2)));
        let later = now + Duration::from_secs(60);
        assert_eq!(p.ask(1, true, later, &three)[2], needed(1));
        p.ask(1, false, later, &three);
        // Not so long after, nor for anyone else.
        let much_later = later + RECENT;
        assert_eq!(p.ask(1, true, much_later, &three)[2], needed(0));
        p.ask(3, false, much_later, &three);
        p.ask(1, false, much_later, &three);
        assert_eq!(p.ask(2, true, much_later, &three)[2], needed(0));
    }

    #[test]
    fn a_pauser_leaving_lets_the_match_go_and_a_voter_leaving_takes_its_vote() {
        let (mut p, now, four) = (Pauses::default(), Instant::now(), people(4, 0));
        p.ask(1, true, now, &four);
        p.ask(2, false, now, &four);
        assert_eq!(p.leave(2), []);
        assert_eq!(p.leave(1), [Action::Pause(false)]);
        // Nobody's pause is left to unpause.
        assert_eq!(p.ask(3, false, now, &four), []);
    }
}
