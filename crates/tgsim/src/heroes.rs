//! Each side's pool of heroes: those it holds, off the map until they
//! join a company.

use tgmap::Id;

use crate::World;

/// A hero a side holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Pooled {
    pub hero: Id,
    /// Its health, as a share of its full health.
    pub health: f32,
    pub experience: f32,
}

crate::hash::digest!(Pooled {
    hero,
    health,
    experience
});

impl World {
    /// The heroes `player` holds, in the order it was given them.
    pub fn heroes(&self, player: u32) -> &[Pooled] {
        self.pools.get(&player).map_or(&[], Vec::as_slice)
    }

    fn holds_hero(&self, player: u32, hero: &Id) -> bool {
        self.heroes(player).iter().any(|h| h.hero == *hero)
    }

    /// Gives `player` the hero `hero`, unharmed, unless it already holds
    /// it; saying whether it did.
    pub fn gain_hero(&mut self, player: u32, hero: &Id) -> bool {
        let known = self.rules.heroes.iter().any(|h| h.id == *hero);
        if !known || self.holds_hero(player, hero) {
            return false;
        }
        let pooled = Pooled {
            hero: hero.clone(),
            health: 1.0,
            experience: 0.0,
        };
        self.pools.entry(player).or_default().push(pooled);
        true
    }

    /// Gives `player` a hero picked at random from those that may be given
    /// so, that fit its faction and that it doesn't hold; saying whether
    /// there was one.
    pub(crate) fn gain_random_hero(&mut self, player: u32) -> bool {
        let faction = self.sides.player(player).and_then(|p| p.faction.as_deref());
        let group = faction.and_then(|f| self.rules.faction_groups.get(&Id::from(f)));
        let fits = |h: &&tgmap::Hero| match (group, &h.group) {
            (Some(g), Some(h)) => g == h,
            _ => true,
        };
        let candidates: Vec<Id> = (self.rules.heroes.iter())
            .filter(|h| h.random && fits(h) && !self.holds_hero(player, &h.id))
            .map(|h| h.id.clone())
            .collect();
        if candidates.is_empty() {
            return false;
        }
        match pick(self.chance(), &candidates) {
            Some(hero) => self.gain_hero(player, &hero),
            None => false,
        }
    }

    /// Takes the hero `hero` from `player`'s pool; saying whether it was
    /// there.
    pub(crate) fn lose_hero(&mut self, player: u32, hero: &Id) -> bool {
        let Some(pool) = self.pools.get_mut(&player) else {
            return false;
        };
        let before = pool.len();
        pool.retain(|h| h.hero != *hero);
        pool.len() < before
    }

    /// Takes a hero picked at random from those in `player`'s pool that may
    /// be given at random; saying whether there was one.
    pub(crate) fn lose_random_hero(&mut self, player: u32) -> bool {
        let random = |id: &Id| self.rules.heroes.iter().any(|h| h.id == *id && h.random);
        let candidates: Vec<Id> = (self.heroes(player).iter())
            .filter(|h| random(&h.hero))
            .map(|h| h.hero.clone())
            .collect();
        if candidates.is_empty() {
            return false;
        }
        match pick(self.chance(), &candidates) {
            Some(hero) => self.lose_hero(player, &hero),
            None => false,
        }
    }
}

/// The one of `from` that `chance`, from 0 up to 1, falls on, each as
/// likely.
fn pick(chance: f32, from: &[Id]) -> Option<Id> {
    let i = ((chance * from.len() as f32) as usize).min(from.len().checked_sub(1)?);
    from.get(i).cloned()
}

#[cfg(test)]
pub(crate) mod tests {
    use tgmap::{Domain, Hero, Id, Pathing};

    use super::pick;
    use crate::World;
    use crate::testing::at_war;

    fn hero(id: &str, random: bool, group: Option<&str>) -> Hero {
        Hero {
            id: Id::from(id),
            random,
            group: group.map(Id::from),
        }
    }

    /// Player 1 leads the royalists, with three heroes of theirs to be
    /// given at random, one of the nationalists' and one of no faction,
    /// and one for scenarios alone.
    pub(crate) fn pool() -> World {
        let mut w = World::from_organizations(at_war(), Pathing::uniform(10, Domain::Land), &[]);
        w.sides.players[0].faction = Some("royalist".into());
        w.rules.heroes = vec![
            hero("andyr", true, Some("group_royalist")),
            hero("arossa", true, Some("group_royalist")),
            hero("azura", true, Some("group_royalist")),
            hero("naava", true, Some("group_nationalist")),
            hero("wanderer", true, None),
            hero("cutscene", false, Some("group_royalist")),
        ];
        (w.rules.faction_groups).insert(Id::from("royalist"), Id::from("group_royalist"));
        w
    }

    fn held(w: &World, player: u32) -> Vec<String> {
        w.heroes(player)
            .iter()
            .map(|h| h.hero.to_string())
            .collect()
    }

    #[test]
    fn a_side_holds_a_named_hero_once() {
        let mut w = pool();
        assert!(w.gain_hero(1, &Id::from("cutscene")));
        assert!(!w.gain_hero(1, &Id::from("cutscene")));
        assert!(!w.gain_hero(1, &Id::from("nobody")));
        assert_eq!(held(&w, 1), ["cutscene"]);
        assert_eq!(w.heroes(1)[0].health, 1.0);
        assert!(w.lose_hero(1, &Id::from("cutscene")));
        assert!(!w.lose_hero(1, &Id::from("cutscene")));
    }

    #[test]
    fn random_heroes_are_of_the_sides_faction_or_none_and_never_held_twice() {
        let mut w = pool();
        while w.gain_random_hero(1) {}
        let mut got = held(&w, 1);
        got.sort();
        assert_eq!(got, ["andyr", "arossa", "azura", "wanderer"]);
        // A side with no faction may have any of them.
        while w.gain_random_hero(2) {}
        assert_eq!(held(&w, 2).len(), 5);
        assert!(w.lose_random_hero(1));
        assert_eq!(held(&w, 1).len(), 3);
    }

    #[test]
    fn picks_fall_evenly_over_the_candidates() {
        let ids: Vec<Id> = ["a", "b", "c"].iter().map(|&i| Id::from(i)).collect();
        assert_eq!(pick(0.0, &ids), Some(Id::from("a")));
        assert_eq!(pick(0.5, &ids), Some(Id::from("b")));
        assert_eq!(pick(0.999, &ids), Some(Id::from("c")));
        assert_eq!(pick(0.5, &[]), None);
    }
}
