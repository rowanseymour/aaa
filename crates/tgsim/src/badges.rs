//! The badge each HQ's division wears, drawn with the match's own chances
//! so every copy of a match gives the same ones: [supply](../../../docs/spec/supply.toml#hq-badge).

use tgmap::Id;

use crate::World;
use crate::hash::digest;

/// The badge an HQ holds, by index into a nation's badges.
#[derive(Debug, Clone, PartialEq)]
pub struct Badge {
    /// The side it was drawn for, once the HQ's been seen in play; none
    /// for one the map stores.
    pub side: Option<u32>,
    /// None where there was none to have.
    pub badge: Option<usize>,
    /// The nation whose badges it's one of, where that isn't the nation
    /// of the HQ's own template, as after it changes side.
    pub nation: Option<String>,
}

digest!(Badge {
    side,
    badge,
    nation
});

impl World {
    /// Gives each HQ that has no badge one, and each that's changed side
    /// another from its new side's nation.
    pub(crate) fn keep_badges(&mut self) {
        let hqs = self.hqs();
        // A lost HQ's badge is free again.
        let standing = |id: &u32| hqs.iter().any(|h| h.id == *id);
        self.badges.retain(|id, b| b.side.is_none() || standing(id));
        for hq in &hqs {
            let own = self.badge_count(self.rules.nations.get(&hq.organization).map(Id::as_str));
            // A badge the map stores is kept only if its nation has that
            // many; otherwise the HQ draws as a new one does.
            let kept = |b: &Badge| b.side.is_some() || b.badge.is_none_or(|b| b < own);
            let held = self.badges.get_mut(&hq.id).filter(|b| kept(b));
            let moved = held
                .as_ref()
                .is_some_and(|b| b.side.is_some_and(|s| s != hq.owner));
            if let Some(held) = held.filter(|_| !moved) {
                held.side = Some(hq.owner);
                continue;
            }
            self.badges.remove(&hq.id);
            let nation = moved
                .then(|| self.sides.player(hq.owner)?.nation.clone())
                .flatten()
                .filter(|n| self.badge_count(Some(n)) > 0);
            let count = match &nation {
                Some(n) => self.badge_count(Some(n)),
                None => own,
            };
            let badge = self.draw_badge(hq.owner, count, &hqs);
            let side = Some(hq.owner);
            self.badges.insert(
                hq.id,
                Badge {
                    side,
                    badge,
                    nation,
                },
            );
        }
    }

    /// One of `count` badges for an HQ of `owner`'s: one at random if no
    /// other of `owner`'s `hqs` holds it, else the first none holds.
    fn draw_badge(&mut self, owner: u32, count: usize, hqs: &[crate::Hq]) -> Option<usize> {
        if count == 0 {
            return None;
        }
        let worn = |w: &World, b: usize| {
            let mut others = hqs.iter().filter(|h| h.owner == owner);
            others.any(|h| {
                w.badges
                    .get(&h.id)
                    .is_some_and(|held| held.badge == Some(b))
            })
        };
        let pick = ((self.chance() * count as f32) as usize).min(count - 1);
        let pick = Some(pick).filter(|&b| !worn(self, b));
        pick.or_else(|| (0..count).find(|&b| !worn(self, b)))
    }

    /// How many badges `nation` has.
    fn badge_count(&self, nation: Option<&str>) -> usize {
        nation
            .and_then(|n| self.rules.badges.get(&Id::from(n)))
            .copied()
            .unwrap_or(0)
    }

    /// The badge the HQ with this id holds.
    pub fn badge(&self, hq: u32) -> Option<&Badge> {
        self.badges.get(&hq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::division;

    /// The division fixture's HQ (id 50, side 1) of a nation with `count`
    /// badges, and a nation `other` of 5 for side 2.
    fn badged(count: usize) -> World {
        let mut w = division();
        let truck = w.organizations[0].template.clone();
        w.rules.nations.insert(truck, Id::from("own"));
        w.rules.badges.insert(Id::from("own"), count);
        w.rules.badges.insert(Id::from("other"), 5);
        let players = &mut w.sides.players;
        players.iter_mut().find(|p| p.id == 2).unwrap().nation = Some("other".into());
        w
    }

    #[test]
    fn a_new_hq_draws_one_of_its_nations_badges() {
        let mut w = badged(20);
        w.tick();
        let held = w.badge(50).cloned().unwrap();
        assert_eq!((held.side, held.nation), (Some(1), None));
        assert!(held.badge.is_some_and(|b| b < 20));
        // It keeps it from then on.
        w.tick();
        assert_eq!(w.badge(50).unwrap().badge, held.badge);
    }

    #[test]
    fn the_draw_follows_the_matchs_chances() {
        let draw = |seed: u64| {
            let mut w = badged(20);
            w.rng = seed;
            w.tick();
            w.badge(50).unwrap().badge
        };
        assert_eq!(draw(7), draw(7));
        assert!(
            (0..20)
                .map(draw)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                > 1
        );
    }

    #[test]
    fn a_held_draw_falls_to_the_first_free() {
        // With one badge, a second HQ of the side has none left.
        let mut w = badged(1);
        w.tick();
        assert_eq!(w.badge(50).unwrap().badge, Some(0));
        let hqs = w.hqs();
        assert_eq!(w.draw_badge(1, 1, &hqs), None);
        // Of two, it's the other.
        w.rules.badges.insert(Id::from("own"), 2);
        assert_eq!(w.draw_badge(1, 2, &hqs), Some(1));
        // Another side's are its own.
        assert_eq!(w.draw_badge(2, 1, &hqs), Some(0));
    }

    #[test]
    fn a_stored_badge_is_kept_if_the_nation_has_it() {
        let mut w = badged(20);
        let stored = |badge| Badge {
            side: None,
            badge: Some(badge),
            nation: None,
        };
        w.badges.insert(50, stored(7));
        w.tick();
        assert_eq!(w.badge(50).unwrap().badge, Some(7));
        let mut w = badged(20);
        w.badges.insert(50, stored(25));
        w.tick();
        assert!(w.badge(50).unwrap().badge.is_some_and(|b| b < 20));
    }

    #[test]
    fn an_hq_that_changes_side_draws_from_its_new_nation() {
        let mut w = badged(20);
        w.tick();
        w.organizations[0].owner = 2;
        w.tick();
        let held = w.badge(50).unwrap();
        assert_eq!(
            (held.side, held.nation.as_deref()),
            (Some(2), Some("other"))
        );
        assert!(held.badge.is_some_and(|b| b < 5));
    }

    #[test]
    fn a_lost_hqs_badge_is_freed() {
        let mut w = badged(20);
        w.tick();
        w.destroy(0);
        w.tick();
        assert!(w.badge(50).is_none());
    }
}
