//! Each side's pool of heroes: those it holds, off the map until they
//! join a company as its captain.

use tgmap::Id;

use crate::{Left, NotYours, Notice, Refused, TICK, World};

/// A hero a side holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Pooled {
    pub hero: Id,
    /// Its health, as a share of its full health, as of when it was last
    /// in the pool.
    pub health: f32,
    pub experience: f32,
    /// Whether it's died and has yet to heal in full.
    pub fallen: bool,
    /// Its unit, by index into [`World::units`], while it leads a company.
    pub unit: Option<usize>,
}

crate::hash::digest!(Pooled {
    hero,
    health,
    experience,
    fallen,
    unit
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
            fallen: false,
            unit: None,
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
        if let Some(r) = self.company_led_by(player, hero) {
            self.release_hero(r);
        }
        let Some(pool) = self.pools.get_mut(&player) else {
            return false;
        };
        let before = pool.len();
        pool.retain(|h| h.hero != *hero);
        pool.len() < before
    }

    /// Whether `player`'s hero `hero` could lead a company now: it's in the
    /// pool, alive and leading none.
    pub fn hero_available(&self, player: u32, hero: &Id) -> bool {
        (self.heroes(player).iter()).any(|h| h.hero == *hero && !h.fallen && h.unit.is_none())
    }

    /// The hero leading the company at `r`, if one does.
    pub fn led_by(&self, r: usize) -> Option<&Pooled> {
        let organization = self.organizations.get(r)?;
        let captain = organization.units[organization.leader?];
        (self.heroes(organization.owner).iter()).find(|h| h.unit == Some(captain))
    }

    /// The company `player`'s hero `hero` leads, if it leads one.
    fn company_led_by(&self, player: u32, hero: &Id) -> Option<usize> {
        let pooled = self.heroes(player).iter().find(|h| h.hero == *hero)?;
        Some(self.units[pooled.unit?].organization)
    }

    pub(crate) fn pooled_mut(&mut self, player: u32, hero: &Id) -> Option<&mut Pooled> {
        (self.pools.get_mut(&player)?.iter_mut()).find(|h| h.hero == *hero)
    }

    /// Whether the company at `r` may take on or let go of a hero now,
    /// giving its captain's place.
    pub fn may_change_captain(&self, r: usize) -> Result<usize, Refused> {
        let organization = &self.organizations[r];
        let place = organization.leader.ok_or(Refused::Unable)?;
        if self.lacks_supply(r) {
            return Err(Refused::Unsettled);
        }
        if !organization.in_supply {
            return Err(Refused::OutOfSupply);
        }
        if self.survivors(r).count() < 2 {
            return Err(Refused::Understrength);
        }
        Ok(place)
    }

    /// Whether `player` may put its hero `hero` at the head of the company at
    /// `r` now, giving the captain's place it would take.
    pub fn may_attach_hero(&self, player: u32, r: usize, hero: &Id) -> Result<usize, Refused> {
        if !self.commands(player, r) {
            return Err(NotYours.into());
        }
        if self.heroes(player).is_empty() {
            return Err(Refused::Unable);
        }
        let place = self.may_change_captain(r)?;
        let pooled = (self.heroes(player).iter()).find(|h| h.hero == *hero);
        let pooled = pooled.ok_or(Refused::Unable)?;
        if pooled.fallen {
            return Err(Refused::Dead);
        }
        match self.company_led_by(player, hero) {
            Some(from) if from == r => return Err(Refused::Unable),
            Some(from) => drop(self.may_change_captain(from)?),
            None => {}
        }
        self.hero_member(hero).ok_or(Refused::Unable)?;
        Ok(place)
    }

    /// Whether `player` may send the hero leading the company at `r` back to
    /// its pool now.
    pub fn may_detach_hero(&self, player: u32, r: usize) -> Result<(), Refused> {
        if !self.commands(player, r) {
            return Err(NotYours.into());
        }
        if self.heroes(player).is_empty() {
            return Err(Refused::Unable);
        }
        self.may_change_captain(r)?;
        self.led_by(r).ok_or(Refused::Unable)?;
        Ok(())
    }

    /// Puts `player`'s hero `hero` at the head of the company at `r`, its
    /// captain leaving the map, unless it's refused.
    pub(crate) fn attach_hero(&mut self, player: u32, r: usize, hero: &Id) -> Result<(), Refused> {
        let place = self.may_attach_hero(player, r, hero)?;
        if let Some(from) = self.company_led_by(player, hero) {
            self.release_hero(from);
        }
        if self.led_by(r).is_some() {
            self.release_hero(r);
        }
        let Some(mut member) = self.hero_member(hero) else {
            return Err(Refused::Unable);
        };
        let health = self.heroes(player).iter().find(|h| h.hero == *hero);
        member.health = health.map_or(1.0, |h| h.health);
        let u = self.replace_member(r, place, member);
        if let Some(pooled) = self.pooled_mut(player, hero) {
            pooled.unit = Some(u);
        }
        Ok(())
    }

    /// Sends the hero leading the company at `r` back to its side's pool,
    /// unless it's refused.
    pub(crate) fn detach_hero(&mut self, player: u32, r: usize) -> Result<(), Refused> {
        self.may_detach_hero(player, r)?;
        self.release_hero(r);
        Ok(())
    }

    /// Takes the hero leading the company at `r` off the map, unharmed, into
    /// its side's pool, the company's own captain to come back as a lost
    /// member does.
    fn release_hero(&mut self, r: usize) {
        let Some(hero) = self.led_by(r).map(|h| h.hero.clone()) else {
            return;
        };
        let organization = &self.organizations[r];
        let (owner, place) = (organization.owner, organization.leader.unwrap_or(0));
        let health = self.health_share(organization.units[place]);
        if let Some(pooled) = self.pooled_mut(owner, &hero) {
            pooled.health = health;
            pooled.unit = None;
        }
        self.restore_captain(r);
    }

    /// The unit at `u`'s health, as a share of its full health.
    fn health_share(&self, u: usize) -> f32 {
        let unit = &self.units[u];
        let full = self.max_health(u);
        match full > 0.0 {
            true => (unit.health / full).clamp(0.0, 1.0),
            false => 1.0,
        }
    }

    /// Gives the captain's place of the company at `r` back to its
    /// template's own captain, lost until resupply brings it.
    fn restore_captain(&mut self, r: usize) {
        let organization = &self.organizations[r];
        let place = organization.leader.unwrap_or(0);
        let raised = self.rules.raisable.get(&organization.template);
        let captain = raised.and_then(|o| o.members.get(o.primary.unwrap_or(0)));
        let Some(mut member) = captain.cloned() else {
            return;
        };
        member.lost = Some(0.0);
        self.replace_member(r, place, member);
    }

    /// The member a hero joins a company as, if it's one that can.
    pub(crate) fn hero_member(&self, hero: &Id) -> Option<tgmap::Member> {
        let raised = self.rules.raisable.get(hero)?;
        raised.members.first().cloned()
    }

    /// Puts a unit standing as `member` in place `p` of the organization at
    /// `r`, returning its index. The unit it replaces, unless it's died
    /// there, leaves the map.
    fn replace_member(&mut self, r: usize, p: usize, mut member: tgmap::Member) -> usize {
        let organization = &self.organizations[r];
        let old = organization.units[p];
        let facing = organization.facing;
        let offsets = &organization.formations[organization.current_formation()].offsets;
        let offset = offsets.get(p).copied().unwrap_or([0.0, 0.0]);
        let place = tgmap::from_local(organization.position, facing, offset);
        let stand = match self.units[old].alive() {
            true => self.units[old].position,
            false => place,
        };
        member.id = self.take_ids(1);
        member.position = stand;
        member.facing = self.units[old].facing;
        let lost = member.lost.is_some();
        let unit = self.new_unit(&member, r);
        let u = self.units.len();
        self.units.push(unit);
        let ticks = self.ticks;
        let replaced = &mut self.units[old];
        if replaced.died.is_none() {
            replaced.left = Some(Left::Replaced(ticks));
            replaced.halt();
        }
        let organization = &mut self.organizations[r];
        organization.units[p] = u;
        organization.replacing[p] = 0.0;
        self.organizations[r].speed = self.slowest(r);
        if !lost {
            self.take_place(u, place, facing, false);
        }
        u
    }

    /// The top speed of the slowest of the organization at `r`'s members.
    fn slowest(&self, r: usize) -> f32 {
        let units = self.organizations[r].units.iter();
        let speed = |u: &usize| {
            let raised = self.rules.raisable.get(&self.units[*u].template)?;
            Some(raised.members.first()?.speed)
        };
        let slowest = units.filter_map(speed).fold(f32::INFINITY, f32::min);
        match slowest.is_finite() {
            true => slowest,
            false => self.organizations[r].speed,
        }
    }

    /// A tick of each side's pool: heroes that died leading a company fall
    /// back into it, any that left the map another way come back to it,
    /// and the first hurt hero in each pool heals.
    pub(crate) fn tend_heroes(&mut self) {
        let mut back = Vec::new();
        for (&player, pool) in &self.pools {
            for h in pool {
                let Some(u) = h.unit else { continue };
                let unit = &self.units[u];
                if unit.died.is_some() || unit.left.is_some() {
                    back.push((player, h.hero.clone(), u, unit.died.is_some()));
                }
            }
        }
        for (player, hero, u, died) in back {
            let r = self.units[u].organization;
            if died {
                if let Some(pooled) = self.pooled_mut(player, &hero) {
                    (pooled.health, pooled.fallen, pooled.unit) = (0.0, true, None);
                }
                let organization = &self.organizations[r];
                if organization.leader.map(|p| organization.units[p]) == Some(u) {
                    self.restore_captain(r);
                }
                (self.notices).push(Notice::HeroKilled {
                    player,
                    hero,
                    organization: r,
                });
            } else {
                let health = self.health_share(u);
                if let Some(pooled) = self.pooled_mut(player, &hero) {
                    (pooled.health, pooled.unit) = (health, None);
                }
            }
        }
        let rates = &self.rules.resupply_rates;
        for pool in self.pools.values_mut() {
            let hurt = pool.iter_mut().find(|h| h.unit.is_none() && h.health < 1.0);
            let Some(h) = hurt else { continue };
            let rate = rates.get(&h.hero).copied().unwrap_or(0.0);
            h.health = (h.health + rate * TICK).min(1.0);
            if h.health >= 1.0 {
                h.fallen = false;
            }
        }
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
    use crate::testing::at_war;
    use crate::{Left, Notice, Refused, World};

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

    /// The village of [`village`](crate::companies::tests::village), in
    /// supply, its companies' members able to fight, and player 1 holding
    /// the hero Andyr, who heals at a tenth of its health a second.
    fn led() -> World {
        let mut w = crate::companies::tests::village();
        let supply = crate::testing::supply(30.0, false, 0.0);
        (w.rules.supplies).insert(Id::from("village"), supply);
        let detached = tgmap::Logistics {
            detached: true,
            ..crate::testing::logistics(None)
        };
        (w.rules.logistics).insert(Id::from("company_pikeman"), detached);
        let company = (w.rules.raisable.get_mut(&Id::from("company_pikeman"))).unwrap();
        for m in &mut company.members {
            m.fighter = Some(crate::testing::soldier(Vec::new()));
        }
        let mut andyr = company.clone();
        andyr.template = Id::from("andyr");
        let mut member = andyr.members[2].clone();
        member.template = Id::from("andyr");
        andyr.members = vec![member];
        andyr.primary = None;
        w.rules.raisable.insert(Id::from("andyr"), andyr);
        w.rules.resupply_rates.insert(Id::from("andyr"), 0.1);
        w.rules.heroes = vec![hero("andyr", true, None)];
        w.gain_hero(1, &Id::from("andyr"));
        w
    }

    fn recruit_led(hero: &str) -> crate::Command {
        crate::Command::Recruit {
            settlement: 0,
            template: Id::from("company_pikeman"),
            hero: Some(Id::from(hero)),
        }
    }

    fn captain(w: &World, r: usize) -> &crate::Unit {
        let organization = &w.organizations[r];
        &w.units[organization.units[organization.leader.unwrap()]]
    }

    fn attach(r: usize) -> crate::Command {
        crate::Command::AttachHero {
            organization: r,
            hero: Id::from("andyr"),
        }
    }

    /// Recruits a plain company and runs until it has a member beside its
    /// captain.
    fn grown(w: &mut World) -> usize {
        let r = w.organizations.len();
        w.apply(1, crate::companies::tests::recruit("company_pikeman"))
            .unwrap();
        crate::testing::run(w, 11.0);
        assert_eq!(w.survivors(r).count(), 2);
        r
    }

    #[test]
    fn a_design_naming_a_hero_comes_out_led_by_it_while_its_available() {
        let mut w = led();
        w.apply(1, recruit_led("andyr")).unwrap();
        assert_eq!(captain(&w, 0).template.as_str(), "andyr");
        assert!(captain(&w, 0).alive());
        assert_eq!(w.led_by(0).unwrap().hero.as_str(), "andyr");
        assert!(!w.hero_available(1, &Id::from("andyr")));
        assert_eq!(w.check(1, &recruit_led("andyr")), Err(Refused::Unable));
        assert_eq!(w.check(1, &recruit_led("nobody")), Err(Refused::Unable));
    }

    #[test]
    fn heroes_join_and_leave_only_companies_in_supply_of_two_or_more() {
        let mut w = led();
        w.apply(1, crate::companies::tests::recruit("company_pikeman"))
            .unwrap();
        crate::testing::run(&mut w, 2.0);
        assert_eq!(w.check(1, &attach(0)), Err(Refused::Understrength));
        crate::testing::run(&mut w, 9.0);
        assert_eq!(w.check(1, &attach(0)), Ok(()));
        let detach = crate::Command::DetachHero { organization: 0 };
        assert_eq!(w.check(1, &detach), Err(Refused::Unable));
        w.organizations[0].in_supply = false;
        assert_eq!(w.check(1, &attach(0)), Err(Refused::Unsettled));
        w.organizations[0].arriving = true;
        assert_eq!(w.check(1, &attach(0)), Err(Refused::OutOfSupply));
        w.organizations[0].arriving = false;
        w.organizations[0].in_supply = true;
        assert_eq!(w.check(2, &attach(0)), Err(Refused::NotYours));
        w.lose_hero(1, &Id::from("andyr"));
        assert_eq!(w.check(1, &attach(0)), Err(Refused::Unable));
    }

    #[test]
    fn a_hero_takes_the_captains_place_and_leaves_it_to_the_captain_again() {
        let mut w = led();
        let r = grown(&mut w);
        let old = w.organizations[r].units[2];
        w.apply(1, attach(r)).unwrap();
        assert_eq!(w.units[old].left, Some(Left::Replaced(w.ticks)));
        assert_eq!(captain(&w, r).template.as_str(), "andyr");
        assert!(captain(&w, r).alive());
        let hero = w.organizations[r].units[2];
        // It comes back to the pool with the health it had.
        let full = w.units[hero].fighter.as_ref().unwrap().health;
        w.units[hero].health = full / 2.0;
        w.apply(1, crate::Command::DetachHero { organization: r })
            .unwrap();
        assert!(w.led_by(r).is_none());
        assert_eq!(w.heroes(1)[0].health, 0.5);
        assert!(w.hero_available(1, &Id::from("andyr")));
        assert!(w.units[hero].left.is_some());
        // The captain comes back as a lost member does.
        assert_eq!(captain(&w, r).template.as_str(), "captain");
        assert!(captain(&w, r).awaited);
        crate::testing::run(&mut w, 11.0);
        assert!(captain(&w, r).alive());
    }

    #[test]
    fn a_hero_moves_straight_from_one_company_to_another() {
        let mut w = led();
        let (a, b) = (grown(&mut w), grown(&mut w));
        w.apply(1, attach(a)).unwrap();
        assert_eq!(w.check(1, &attach(a)), Err(Refused::Unable));
        w.apply(1, attach(b)).unwrap();
        assert_eq!(captain(&w, a).template.as_str(), "captain");
        assert_eq!(captain(&w, b).template.as_str(), "andyr");
        assert_eq!(w.units[w.heroes(1)[0].unit.unwrap()].organization, b);
    }

    #[test]
    fn a_hero_killed_falls_back_into_the_pool_and_heals_there() {
        let mut w = led();
        let (r, other) = (grown(&mut w), grown(&mut w));
        w.apply(1, attach(r)).unwrap();
        let hero = w.organizations[r].units[2];
        let ticks = w.ticks;
        w.units[hero].die(ticks);
        w.tick();
        let killed = Notice::HeroKilled {
            player: 1,
            hero: Id::from("andyr"),
            organization: r,
        };
        assert!(w.notices.contains(&killed));
        assert!(w.heroes(1)[0].fallen && w.heroes(1)[0].unit.is_none());
        assert!(w.heroes(1)[0].health < 0.05);
        assert_eq!(captain(&w, r).template.as_str(), "captain");
        // The body stays where it fell.
        assert!(w.units[hero].left.is_none());
        assert_eq!(w.check(1, &attach(other)), Err(Refused::Dead));
        assert_eq!(w.check(1, &recruit_led("andyr")), Err(Refused::Unable));
        crate::testing::run(&mut w, 9.5);
        assert!(!w.hero_available(1, &Id::from("andyr")));
        crate::testing::run(&mut w, 0.5);
        assert!(w.hero_available(1, &Id::from("andyr")));
        assert_eq!(w.heroes(1)[0].health, 1.0);
    }

    #[test]
    fn a_hero_leaves_with_its_company_or_its_side_and_its_captain_returns() {
        let mut w = led();
        let r = grown(&mut w);
        w.apply(1, attach(r)).unwrap();
        let hero = w.organizations[r].units[2];
        let full = w.units[hero].fighter.as_ref().unwrap().health;
        w.units[hero].health = full * 0.25;
        w.disband(r);
        w.tick();
        assert_eq!(w.heroes(1)[0].unit, None);
        assert!(w.heroes(1)[0].health < 0.27);
        assert!(!w.heroes(1)[0].fallen);
        let other = grown(&mut w);
        w.apply(1, attach(other)).unwrap();
        assert!(w.lose_hero(1, &Id::from("andyr")));
        assert_eq!(captain(&w, other).template.as_str(), "captain");
        assert!(captain(&w, other).awaited);
    }

    #[test]
    fn a_wounded_hero_comes_out_with_the_health_it_has() {
        let mut w = led();
        w.pools.get_mut(&1).unwrap()[0].health = 0.5;
        w.apply(1, recruit_led("andyr")).unwrap();
        let unit = captain(&w, 0);
        assert_eq!(unit.health, unit.fighter.as_ref().unwrap().health * 0.5);
    }
}
