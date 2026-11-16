//! How the match comes out for each side: what defeats one, what wins it,
//! and how either settles the rest.

use std::collections::BTreeSet;

use crate::{Notice, Outcome, SECOND, State, World};

/// Ticks played before sides are first judged, which gives them time to
/// be given what they start with.
const GRACE: u64 = SECOND;

/// Where a side stands in the match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fate {
    pub outcome: Outcome,
    /// Whether having nothing left that keeps it in the match defeats it,
    /// which a map can turn off to decide the side's outcome itself.
    pub default_defeat: bool,
    /// Whether it won by the share of the map's cities its team held.
    pub by_cities: bool,
}

impl World {
    /// How the match has come out for a side; still playing for one the
    /// match doesn't have.
    pub fn outcome(&self, player: u32) -> Outcome {
        self.fates
            .get(&player)
            .map_or(Outcome::Playing, |f| f.outcome)
    }

    /// Has a side win the match, as a map's triggers can; nothing for one
    /// whose outcome is decided.
    pub fn win(&mut self, player: u32) {
        self.decide(player, true, true);
    }

    /// Has a side lose the match, as a map's triggers can; nothing for one
    /// whose outcome is decided.
    pub fn lose(&mut self, player: u32) {
        self.decide(player, false, true);
    }

    /// Sets whether having nothing left defeats a side.
    pub fn set_default_defeat(&mut self, player: u32, on: bool) {
        if let Some(fate) = self.fates.get_mut(&player) {
            fate.default_defeat = on;
        }
    }

    /// Whether a side has nothing that keeps it in the match, whatever
    /// its outcome and whether or not that defeats it.
    pub fn has_nothing_left(&self, player: u32) -> bool {
        !self.kept_in(player, &self.sustained())
    }

    /// Defeats the sides with nothing left and has those holding enough
    /// cities win, one after another in side order, each seeing what the
    /// earlier ones left: of the last two sides losing everything on one
    /// tick, the earlier is defeated and the later wins.
    pub(crate) fn settle_outcomes(&mut self) {
        // Once it's over every outcome is decided, for good.
        if self.over || self.ticks < GRACE {
            return;
        }
        let ids: Vec<u32> = self.sides.players.iter().map(|p| p.id).collect();
        let sustained = self.sustained();
        for &player in &ids {
            if self.over {
                break;
            }
            let Some(&fate) = self.fates.get(&player) else {
                continue;
            };
            let playing = fate.outcome == Outcome::Playing;
            if playing && fate.default_defeat && !self.kept_in(player, &sustained) {
                self.lose(player);
            }
        }
        let ratio = self.rules.city_victory_ratio as usize;
        let centres = (self.buildings.iter()).filter(|b| b.zone.is_some() && !b.gone());
        let cities: Vec<u32> = centres.map(|b| b.owner).collect();
        if ratio == 0 || cities.is_empty() {
            return;
        }
        for &player in &ids {
            let independent = self.sides.player(player).is_some_and(|p| p.independent);
            if independent || self.outcome(player) != Outcome::Playing {
                continue;
            }
            // Its team's count however the team's sides have fared, and
            // whether or not teams win together.
            let held = cities.iter().filter(|&&o| self.team_mates(o, player));
            let held = held.count();
            if held * 100 < ratio * cities.len() {
                continue;
            }
            self.notices.push(Notice::CitiesHeld {
                player,
                cities: held,
            });
            for &mate in &ids {
                let mates = self.team_mates(mate, player);
                if let Some(fate) = self.fates.get_mut(&mate).filter(|_| mates) {
                    fate.by_cities = true;
                }
            }
            self.win(player);
        }
    }

    /// The sides that own something that keeps a side in the match: a
    /// organization, or a building or holding of a
    /// [sustaining](crate::Rules::sustaining) template.
    fn sustained(&self) -> BTreeSet<u32> {
        let sustaining = &self.rules.sustaining;
        let organizations = (0..self.organizations.len())
            .filter(|&r| !self.destroyed(r))
            .filter(|&r| !self.lone(r) || sustaining.contains(&self.organizations[r].template))
            .map(|r| self.organizations[r].owner);
        let buildings = (self.buildings.iter())
            .filter(|b| b.state == State::Standing && sustaining.contains(&b.template))
            .map(|b| b.owner);
        // What's neither is never lost, so keeps its side in for good.
        let held = (self.holdings.iter())
            .filter(|(_, template)| sustaining.contains(template))
            .map(|&(owner, _)| owner);
        organizations.chain(buildings).chain(held).collect()
    }

    /// Whether a side is kept in the match by what the `sustained` sides
    /// own: its own things, or where teams win together its team-mates',
    /// so that no side of a team falls before the rest.
    fn kept_in(&self, player: u32, sustained: &BTreeSet<u32>) -> bool {
        let shared = self.rules.team_victory;
        (sustained.iter()).any(|&s| s == player || shared && self.team_mates(s, player))
    }

    fn team(&self, player: u32) -> Option<usize> {
        self.sides.player(player)?.team
    }

    /// Whether two sides are one, or on one team. How they stand toward
    /// each other doesn't come into a match's outcome.
    fn team_mates(&self, a: u32, b: u32) -> bool {
        a == b || self.team(a).is_some() && self.team(a) == self.team(b)
    }

    /// The team a side wins and loses with: its own, where teams win
    /// together and it has others on it. A side without one stands alone.
    fn shared_team(&self, player: u32) -> Option<usize> {
        let team = self.team(player).filter(|_| self.rules.team_victory)?;
        let members = self.sides.players.iter().filter(|p| p.team == Some(team));
        (members.count() > 1).then_some(team)
    }

    /// Decides the outcome of a side still playing, telling of it if the
    /// side stands alone, and if `spreads` settling what it means for the
    /// rest. An independent side's outcome is nobody else's.
    fn decide(&mut self, player: u32, won: bool, spreads: bool) {
        let Some(fate) = self.fates.get_mut(&player) else {
            return;
        };
        if fate.outcome != Outcome::Playing {
            return;
        }
        fate.outcome = if won { Outcome::Won } else { Outcome::Lost };
        if self.shared_team(player).is_none() {
            match won {
                true => self.notices.push(Notice::Victorious { player }),
                // Those who lose to another's win aren't told of one by one.
                false if !self.over => self.notices.push(Notice::Defeated { player }),
                false => {}
            }
        }
        let independent = self.sides.player(player).is_some_and(|p| p.independent);
        match won {
            _ if !spreads || independent => {}
            true => self.spread_win(player),
            false => self.spread_loss(player),
        }
    }

    /// Ends the match on a side's win: its team wins with it where teams
    /// win together, and everyone else loses.
    fn spread_win(&mut self, player: u32) {
        self.over = true;
        let team = self.shared_team(player);
        if let Some(team) = team {
            self.notices.push(Notice::TeamVictorious { team });
        }
        let ids: Vec<u32> = self.sides.players.iter().map(|p| p.id).collect();
        for other in ids {
            let with = team.is_some() && self.team(other) == team;
            self.decide(other, with, false);
        }
    }

    /// Ends the match on a side's loss if that leaves nobody, one side or
    /// the sides of one team, who then win whether or not teams win
    /// together. Otherwise the match goes on.
    fn spread_loss(&mut self, player: u32) {
        let ids = self.sides.players.iter().map(|p| p.id);
        let left: Vec<u32> = ids.filter(|&p| self.outcome(p) != Outcome::Lost).collect();
        if left.iter().any(|&p| !self.team_mates(p, left[0])) {
            let Some(team) = self.shared_team(player) else {
                return;
            };
            let mut members = self.sides.players.iter().filter(|p| p.team == Some(team));
            if members.all(|p| self.outcome(p.id) == Outcome::Lost) {
                self.notices.push(Notice::TeamDefeated { team });
            }
            return;
        }
        if let Some(&first) = left.first() {
            self.decide(first, true, false);
            if let Some(team) = self.shared_team(first) {
                self.notices.push(Notice::TeamVictorious { team });
            }
            for &mate in &left[1..] {
                self.decide(mate, true, false);
            }
        }
        self.over = true;
    }
}

crate::hash::digest!(Fate {
    outcome,
    default_defeat,
    by_cities
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;
    use crate::{Player, Sides};
    use Outcome::*;
    use tgmap::{Domain, Id, Pathing};

    /// Sides 1 and 2 on one team and 3 and 4 on another, each with a
    /// company, and an independent 9 with nothing whose loss decides
    /// nothing.
    fn teams() -> World {
        let team = |id| crate::Team {
            id,
            name: format!("team_{id}"),
            relations: Vec::new(),
            team_relations: Vec::new(),
        };
        let side = |id, team| player(id, team, &[], &[]);
        let sides = Sides {
            players: vec![
                side(1, Some(0)),
                side(2, Some(0)),
                side(3, Some(1)),
                side(4, Some(1)),
                side(9, None),
            ],
            teams: vec![team(10), team(11)],
        };
        let mut w = World::from_organizations(sides, land(), &[]);
        for owner in 1..=4 {
            company(&mut w, owner);
        }
        w
    }

    /// Sides 1, 2 and 3, each alone and with a company.
    fn free_for_all() -> World {
        let players = (1..=3).map(|id| {
            let mut p = player(id, None, &[], &[]);
            p.independent = false;
            p
        });
        let sides = Sides {
            players: players.collect(),
            teams: Vec::new(),
        };
        let mut w = World::from_organizations(sides, land(), &[]);
        for owner in 1..=3 {
            company(&mut w, owner);
        }
        w
    }

    fn land() -> Pathing {
        Pathing::uniform(40, Domain::Land)
    }

    fn outcomes(w: &World) -> Vec<Outcome> {
        let ids = w.sides.players.iter().map(|p| p.id);
        ids.map(|p| w.outcome(p)).collect()
    }

    /// Kills every organization of `owner`.
    fn rout(w: &mut World, owner: u32) {
        for r in 0..w.organizations.len() {
            if w.organizations[r].owner == owner {
                w.destroy(r);
            }
        }
    }

    fn told(w: &mut World) -> Vec<Notice> {
        let judged = |n: &Notice| {
            matches!(
                n,
                Notice::Defeated { .. }
                    | Notice::Victorious { .. }
                    | Notice::TeamDefeated { .. }
                    | Notice::TeamVictorious { .. }
                    | Notice::CitiesHeld { .. }
            )
        };
        w.take_notices().into_iter().filter(judged).collect()
    }

    #[test]
    fn nobody_is_judged_in_the_first_second() {
        let mut w = free_for_all();
        rout(&mut w, 3);
        run(&mut w, 0.9);
        assert_eq!(outcomes(&w), [Playing; 3]);
        w.tick();
        assert_eq!(outcomes(&w), [Playing, Playing, Lost]);
        assert!(!w.over);
        assert_eq!(told(&mut w), [Notice::Defeated { player: 3 }]);
    }

    #[test]
    fn the_last_side_standing_wins_and_the_match_is_over() {
        let mut w = free_for_all();
        run(&mut w, 2.0);
        rout(&mut w, 1);
        w.tick();
        assert!(!w.over);
        rout(&mut w, 3);
        w.tick();
        assert!(w.over);
        assert_eq!(outcomes(&w), [Lost, Won, Lost]);
        let events = [
            Notice::Defeated { player: 1 },
            Notice::Defeated { player: 3 },
            Notice::Victorious { player: 2 },
        ];
        assert_eq!(told(&mut w), events);
    }

    #[test]
    fn of_the_last_two_falling_together_the_later_wins() {
        let mut w = free_for_all();
        run(&mut w, 2.0);
        for owner in 1..=3 {
            rout(&mut w, owner);
        }
        w.tick();
        assert_eq!(outcomes(&w), [Lost, Lost, Won]);
        // A decided outcome stays, whatever the side then has.
        run(&mut w, 2.0);
        assert_eq!(outcomes(&w), [Lost, Lost, Won]);
    }

    #[test]
    fn only_organizations_and_sustaining_templates_keep_a_side_in() {
        let mut w = free_for_all();
        let depot = building(50, 1, [20.0, 20.0]);
        let hq = tgmap::Building {
            template: Id::from("hq"),
            ..building(51, 2, [40.0, 20.0])
        };
        w = with_buildings(w, &[depot, hq]);
        w.rules.sustaining.insert(Id::from("hq"));
        rout(&mut w, 1);
        rout(&mut w, 2);
        assert!(w.has_nothing_left(1));
        assert!(!w.has_nothing_left(2));
        run(&mut w, 1.0);
        assert_eq!(outcomes(&w), [Lost, Playing, Playing]);
        // Nor does a side's holding of such a template let it fall.
        w.holdings.push((3, Id::from("hq")));
        rout(&mut w, 3);
        w.tick();
        assert_eq!(outcomes(&w), [Lost, Playing, Playing]);
    }

    #[test]
    fn a_match_takes_up_the_outcomes_its_map_was_saved_with() {
        let side = |id, outcome| Player {
            outcome,
            ..player(id, Some(0), &[], &[])
        };
        let sides = |outcomes: [Outcome; 2]| Sides {
            players: vec![side(1, outcomes[0]), side(2, outcomes[1])],
            teams: Vec::new(),
        };
        let world = |outcomes| World::from_organizations(sides(outcomes), land(), &[]);
        let mut w = world([Lost, Won]);
        assert!(w.over);
        run(&mut w, 2.0);
        assert_eq!(outcomes(&w), [Lost, Won]);
        assert!(w.take_notices().is_empty());
        assert!(!world([Lost, Playing]).over);
        assert!(world([Lost, Lost]).over);
    }

    #[test]
    fn a_side_the_match_was_set_up_without_is_never_judged() {
        let mut w = free_for_all();
        w.sides.players.push(player(7, None, &[], &[]));
        run(&mut w, 2.0);
        assert_eq!(w.outcome(7), Playing);
    }

    #[test]
    fn a_team_falls_together_once_none_of_it_has_anything() {
        let mut w = teams();
        run(&mut w, 1.0);
        // The independent side has nothing, and its loss ends nothing.
        assert_eq!(outcomes(&w), [Playing, Playing, Playing, Playing, Lost]);
        assert_eq!(told(&mut w), [Notice::Defeated { player: 9 }]);
        rout(&mut w, 3);
        w.tick();
        assert_eq!(w.outcome(3), Playing);
        rout(&mut w, 4);
        w.tick();
        assert!(w.over);
        assert_eq!(outcomes(&w), [Won, Won, Lost, Lost, Lost]);
        assert_eq!(told(&mut w), [Notice::TeamVictorious { team: 0 }]);
    }

    #[test]
    fn without_team_victory_sides_fall_alone_but_a_team_left_still_wins() {
        let mut w = teams();
        w.rules.team_victory = false;
        run(&mut w, 1.0);
        told(&mut w);
        rout(&mut w, 3);
        w.tick();
        assert_eq!(w.outcome(3), Lost);
        assert!(!w.over);
        rout(&mut w, 4);
        w.tick();
        assert!(w.over);
        assert_eq!(outcomes(&w), [Won, Won, Lost, Lost, Lost]);
        let events = [
            Notice::Defeated { player: 3 },
            Notice::Defeated { player: 4 },
            Notice::Victorious { player: 1 },
            Notice::Victorious { player: 2 },
        ];
        assert_eq!(told(&mut w), events);
    }

    #[test]
    fn a_team_defeated_is_told_of_only_while_the_match_goes_on() {
        let mut w = teams();
        let third = crate::Team {
            id: 12,
            name: "team_12".into(),
            relations: Vec::new(),
            team_relations: Vec::new(),
        };
        w.sides.teams.push(third);
        for id in [5, 6] {
            w.sides.players.push(player(id, Some(2), &[], &[]));
            let fate = w.fates[&1];
            w.fates.insert(id, fate);
            company(&mut w, id);
        }
        run(&mut w, 1.0);
        told(&mut w);
        rout(&mut w, 3);
        rout(&mut w, 4);
        w.tick();
        assert!(!w.over);
        assert_eq!(told(&mut w), [Notice::TeamDefeated { team: 1 }]);
        rout(&mut w, 5);
        rout(&mut w, 6);
        w.tick();
        assert!(w.over);
        assert_eq!(told(&mut w), [Notice::TeamVictorious { team: 0 }]);
    }

    #[test]
    fn a_side_with_default_defeat_off_stays_in_with_nothing() {
        let mut w = free_for_all();
        w.set_default_defeat(3, false);
        rout(&mut w, 3);
        rout(&mut w, 1);
        run(&mut w, 2.0);
        assert_eq!(outcomes(&w), [Lost, Playing, Playing]);
        assert!(w.has_nothing_left(3));
        w.set_default_defeat(3, true);
        w.tick();
        assert_eq!(outcomes(&w), [Lost, Won, Lost]);
    }

    #[test]
    fn a_win_by_trigger_takes_the_team_and_tells_of_no_losers() {
        let mut w = teams();
        w.win(2);
        assert!(w.over);
        assert_eq!(outcomes(&w), [Won, Won, Lost, Lost, Lost]);
        assert_eq!(told(&mut w), [Notice::TeamVictorious { team: 0 }]);
        // Decided once, for good.
        w.lose(1);
        assert_eq!(w.outcome(1), Won);
    }

    #[test]
    fn the_one_left_wins_even_if_independent_and_nobody_left_ends_it_unwon() {
        let mut w = free_for_all();
        w.sides.players[0].independent = true;
        w.lose(2);
        assert!(!w.over);
        w.lose(3);
        assert!(w.over);
        assert_eq!(outcomes(&w), [Won, Lost, Lost]);
        let mut w = free_for_all();
        w.sides.players.truncate(1);
        w.lose(1);
        assert!(w.over);
        assert_eq!(outcomes(&w), [Lost]);
    }

    #[test]
    fn holding_the_share_of_cities_wins_for_the_team() {
        let mut w = teams();
        let centre = |id, owner, x| tgmap::Building {
            zone: Some(10.0),
            ..building(id, owner, [x, 60.0])
        };
        let cities = [
            centre(50, 1, 10.0),
            centre(51, 2, 30.0),
            centre(52, 3, 50.0),
            centre(53, 9, 70.0),
        ];
        w = with_buildings(w, &cities);
        w.rules.city_victory_ratio = 60;
        run(&mut w, 1.0);
        assert!(!w.over);
        told(&mut w);
        // A city that's gone is nobody's, and no longer one of the map's.
        w.rules.city_victory_ratio = 50;
        w.buildings[1].state = State::Destroyed(w.ticks);
        w.tick();
        assert!(!w.over);
        w.buildings[1].state = State::Standing;
        w.tick();
        assert!(w.over);
        assert_eq!(outcomes(&w), [Won, Won, Lost, Lost, Lost]);
        assert!(w.fates[&1].by_cities && w.fates[&2].by_cities);
        assert!(!w.fates[&3].by_cities);
        let events = [
            Notice::CitiesHeld {
                player: 1,
                cities: 2,
            },
            Notice::TeamVictorious { team: 0 },
        ];
        assert_eq!(told(&mut w), events);
    }
}
