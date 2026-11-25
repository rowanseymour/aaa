//! Companies recruited at settlements: made at once, at the settlement,
//! with only their captain, the rest coming as resupply brings them.

use std::borrow::Cow;

use tgmap::Id;

use crate::{NotYours, Notice, Refused, World};

impl World {
    /// The index into [`World::settlements`] of the settlement whose
    /// centre is the building at `building`.
    pub fn settlement_of(&self, building: usize) -> Option<usize> {
        let id = self.buildings.get(building)?.id;
        self.settlements.iter().position(|s| s.centre == id)
    }

    /// The index into [`World::buildings`] of the settlement at
    /// `settlement`'s centre, while it stands.
    pub fn centre_of(&self, settlement: usize) -> Option<usize> {
        let id = self.settlements.get(settlement)?.centre;
        (self.buildings.iter()).position(|b| b.id == id && b.standing())
    }

    /// Whether the building at `building` is the centre of a settlement
    /// that recruits companies now: one that lists some and isn't under
    /// siege.
    pub fn recruits_companies(&self, building: usize) -> bool {
        (self.settlement_of(building)).is_some_and(|s| !self.company_list(s).is_empty())
            && self.buildings[building].besieged.is_none()
    }

    /// The company templates the settlement at `settlement` lists, in its
    /// menu's order, whether or not it can recruit them yet.
    pub fn company_list(&self, settlement: usize) -> &[Id] {
        (self.settlements.get(settlement))
            .and_then(|s| self.rules.recruit_lists.get(&s.template))
            .map_or(&[], Vec::as_slice)
    }

    /// What recruiting a company of `template`, led by `hero` if one is
    /// named, at the settlement at `settlement` would recruit for
    /// `player`, unless it's refused.
    pub(crate) fn may_recruit(
        &self,
        player: u32,
        settlement: usize,
        template: &Id,
        hero: Option<&Id>,
    ) -> Result<Cow<'_, tgmap::Organization>, Refused> {
        (self.settlements.get(settlement))
            .filter(|s| s.owner == player)
            .ok_or(NotYours)?;
        let centre = self.centre_of(settlement).ok_or(NotYours)?;
        let listed = self.company_list(settlement).contains(template);
        let organization = (self.rules.raisable.get(template))
            .filter(|_| listed)
            .ok_or(Refused::Unable)?;
        let organization = match hero {
            Some(hero) => Cow::Owned(self.led_by_hero(player, organization, hero)?),
            None => Cow::Borrowed(organization),
        };
        if !self.lacks(player, centre, &organization).is_empty() {
            return Err(Refused::Lacking);
        }
        if self.buildings[centre].besieged.is_some() {
            return Err(Refused::Besieged);
        }
        if self.cost(centre, &organization) > self.stockpile(player) {
            return Err(Refused::Unaffordable);
        }
        if self.over_limit(player, &organization).is_some() {
            return Err(Refused::OverLimit);
        }
        Ok(organization)
    }

    /// `organization` with `player`'s hero `hero` in its captain's place,
    /// refused unless the hero is available.
    fn led_by_hero(
        &self,
        player: u32,
        organization: &tgmap::Organization,
        hero: &Id,
    ) -> Result<tgmap::Organization, Refused> {
        if !self.hero_available(player, hero) {
            return Err(Refused::Unable);
        }
        let place = organization.primary.ok_or(Refused::Unable)?;
        let mut led = organization.clone();
        let captain = led.members.get_mut(place).ok_or(Refused::Unable)?;
        let mut member = self.hero_member(hero).ok_or(Refused::Unable)?;
        let health = self.heroes(player).iter().find(|h| h.hero == *hero);
        member.health = health.map_or(1.0, |h| h.health);
        (member.id, member.position, member.facing) =
            (captain.id, captain.position, captain.facing);
        *captain = member;
        Ok(led)
    }

    /// Recruits a company of `template` at the settlement at `settlement`
    /// for `player`, paid for at once. One with nowhere to come out is
    /// refunded, and its side told.
    pub(crate) fn recruit(
        &mut self,
        player: u32,
        settlement: usize,
        template: &Id,
        hero: Option<&Id>,
    ) -> Result<(), Refused> {
        let mut organization = self
            .may_recruit(player, settlement, template, hero)?
            .into_owned();
        let centre = self.centre_of(settlement).ok_or(NotYours)?;
        let paid = self.cost(centre, &organization);
        let left = self.stockpile(player) - paid;
        self.set_stockpile(player, left);
        organization.owner = player;
        let Some(r) = self.come_out(centre, organization) else {
            self.refund(player, paid);
            self.notices.push(Notice::NotCreated {
                player,
                template: template.clone(),
            });
            return Ok(());
        };
        // On to the settlement's rally point, as a building's recruits go.
        self.organizations[r].rally = self.buildings[centre].rally;
        if let Some(hero) = hero {
            let organization = &self.organizations[r];
            let captain = organization.leader.map(|p| organization.units[p]);
            if let Some(pooled) = self.pooled_mut(player, hero) {
                pooled.unit = captain;
            }
        }
        log::debug!(
            "{} {} recruited at {} {}",
            self.organizations[r].template,
            self.organizations[r].id,
            self.buildings[centre].template,
            self.buildings[centre].id
        );
        self.happen(crate::triggers::Happening::Recruited {
            player,
            template: self.organizations[r].template.clone(),
        });
        self.notices.push(Notice::PutOut {
            player,
            building: centre,
            organization: r,
        });
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::testing::*;

    use tgmap::{Domain, Id, Obstacle, Pathing, Requires, Shape};

    use crate::{Command, Notice, Refused, World, distance};

    /// Player 1's village centre at 20,20 with its settlement, recruiting
    /// a company of two pikemen and a captain for 8 a pikeman, with room
    /// under the village's limit for two companies, and 500 to spend.
    pub(crate) fn village() -> World {
        let mut centre = building(50, 1, [20.0, 20.0]);
        centre.template = "village".into();
        let mut w = with_buildings(
            World::from_organizations(at_war(), Pathing::uniform(40, Domain::Land), &[]),
            &[centre],
        );
        w.settlements.push(tgmap::Settlement {
            id: 60,
            template: Id::from("settlement"),
            owner: 1,
            centre: 50,
        });
        let unit = |id, template: &str, x| {
            let mut m = member(id, [x, 0.0], 3.0);
            m.template = template.into();
            m.shape = Some(Shape::Circle { radius: 0.5 });
            m
        };
        let mut company = raised_company(&mut w, 1);
        company.template = "company_pikeman".into();
        company.members = vec![
            unit(1, "pikeman", -1.5),
            unit(2, "pikeman", 1.5),
            unit(3, "captain", 0.0),
        ];
        company.primary = Some(2);
        company.formations[0].offsets = vec![[-1.5, 1.0], [1.5, 1.0], [0.0, 0.0]];
        let economy = |cost, upkeep| tgmap::Economy {
            cost: amounts(cost),
            upkeep: amounts(upkeep),
            ..Default::default()
        };
        let rules = &mut w.rules;
        rules.economies.insert(
            Id::from("company_pikeman"),
            economy(&[], &[("unit_limit_consumed", 1.0)]),
        );
        let pikeman = economy(&[("money", 8.0)], &[]);
        rules.economies.insert(Id::from("pikeman"), pikeman);
        let provides = tgmap::Economy {
            production: amounts(&[("unit_limit_provided", 2.0)]),
            ..Default::default()
        };
        rules.economies.insert(Id::from("village"), provides);
        for t in ["pikeman", "captain"] {
            rules.resupply_rates.insert(Id::from(t), 0.1);
        }
        rules.economy = tgmap::EconomyRules {
            resources: aa_resources(),
            limited_max: 20.0,
            stockpile_max: 99999.0,
            ..Default::default()
        };
        (rules.recruit_lists).insert(Id::from("settlement"), vec![Id::from("company_pikeman")]);
        rules.raisable.insert(Id::from("company_pikeman"), company);
        (rules.logistics).insert(Id::from("company_pikeman"), logistics(None));
        for p in 1..=3 {
            w.set_default_defeat(p, false);
        }
        w.set_stockpile(1, 500.0);
        w
    }

    pub(crate) fn recruit(template: &str) -> Command {
        Command::Recruit {
            settlement: 0,
            template: Id::from(template),
            hero: None,
        }
    }

    #[test]
    fn a_company_comes_out_at_once_by_its_settlement_with_only_its_captain() {
        let mut w = village();
        w.apply(1, recruit("company_pikeman")).unwrap();
        assert_eq!(w.organizations.len(), 1);
        assert_eq!(w.stockpile(1), 484.0);
        let put_out = Notice::PutOut {
            player: 1,
            building: 0,
            organization: 0,
        };
        assert!(w.notices.contains(&put_out));
        let r = &w.organizations[0];
        assert_eq!(r.owner, 1);
        let awaited: Vec<bool> = r.units.iter().map(|&u| w.units[u].awaited).collect();
        assert_eq!(awaited, [true, true, false]);
        // The captain stands clear of the centre, facing the way it does.
        let captain = &w.units[r.units[2]];
        assert!(distance(captain.position, [20.0, 20.0]) > 3.5);
        assert!(distance(captain.position, [20.0, 20.0]) < 12.0);
        assert_eq!(r.facing, [0.0, -1.0]);
    }

    #[test]
    fn a_company_that_isnt_supplied_comes_out_whole() {
        let mut w = village();
        w.rules.logistics.remove(&Id::from("company_pikeman"));
        w.apply(1, recruit("company_pikeman")).unwrap();
        let r = &w.organizations[0];
        assert!(r.units.iter().all(|&u| !w.units[u].awaited));
    }

    #[test]
    fn a_company_recruited_full_comes_out_whole() {
        let mut w = village();
        let logistics = w
            .rules
            .logistics
            .get_mut(&Id::from("company_pikeman"))
            .unwrap();
        logistics.recruit_full = true;
        w.apply(1, recruit("company_pikeman")).unwrap();
        let r = &w.organizations[0];
        assert!(r.units.iter().all(|&u| !w.units[u].awaited));
    }

    #[test]
    fn a_settlement_sends_its_companies_to_its_rally_point_while_it_can_recruit() {
        let mut w = village();
        let rally = |at| Command::Rally {
            building: 0,
            at: Some(at),
        };
        w.apply(1, rally([30.0, 30.0])).unwrap();
        w.apply(1, recruit("company_pikeman")).unwrap();
        assert_eq!(w.organizations[0].rally, Some([30.0, 30.0]));
        let r = &w.organizations[0];
        let captain = r.units[r.leader.unwrap()];
        let before = distance(w.units[captain].position, [30.0, 30.0]);
        run(&mut w, 2.0);
        assert!(distance(w.units[captain].position, [30.0, 30.0]) < before);
        // Not while it's under siege, as it can't recruit then.
        w.buildings[0].besieged = Some(w.ticks + 100);
        assert_eq!(w.apply(1, rally([10.0, 10.0])), Err(Refused::Unable));
        assert_eq!(w.buildings[0].rally, Some([30.0, 30.0]));
    }

    #[test]
    fn the_leader_stands_where_room_was_found_whatever_its_place() {
        let mut w = village();
        let company = w
            .rules
            .raisable
            .get_mut(&Id::from("company_pikeman"))
            .unwrap();
        company.formations[0].offsets[2] = [0.0, -2.0];
        w.apply(1, recruit("company_pikeman")).unwrap();
        let r = &w.organizations[0];
        let captain = w.units[r.units[2]].position;
        // The first point tried with room is straight out ahead of the
        // centre, facing -y, clear of its 6-wide footprint.
        assert_eq!(captain, [20.0, 16.0]);
    }

    #[test]
    fn recruiting_is_refused_for_what_the_settlement_cant_recruit_now() {
        let mut w = village();
        assert_eq!(
            w.apply(2, recruit("company_pikeman")),
            Err(Refused::NotYours)
        );
        assert_eq!(w.apply(1, recruit("company_bowman")), Err(Refused::Unable));
        w.set_stockpile(1, 10.0);
        assert_eq!(
            w.apply(1, recruit("company_pikeman")),
            Err(Refused::Unaffordable)
        );
        w.set_stockpile(1, 500.0);
        w.buildings[0].besieged = Some(w.ticks + 100);
        assert_eq!(
            w.apply(1, recruit("company_pikeman")),
            Err(Refused::Besieged)
        );
        w.buildings[0].besieged = None;
        // The village has room for two.
        w.apply(1, recruit("company_pikeman")).unwrap();
        w.apply(1, recruit("company_pikeman")).unwrap();
        assert_eq!(
            w.apply(1, recruit("company_pikeman")),
            Err(Refused::OverLimit)
        );
        assert_eq!(w.stockpile(1), 468.0);
    }

    #[test]
    fn a_settlement_counts_its_own_structures_and_those_in_none() {
        let mut w = village();
        w.rules.requires.insert(
            Id::from("pikeman"),
            Requires {
                structures: vec![vec![Id::from("woodmill")]],
                ..Requires::default()
            },
        );
        let recruited = |w: &mut World| w.apply(1, recruit("company_pikeman"));
        assert_eq!(recruited(&mut w), Err(Refused::Lacking));
        let mut mill = building(70, 1, [5.0, 35.0]);
        mill.template = "woodmill".into();
        w = with_buildings(w, &[mill]);
        assert_eq!(recruited(&mut w), Ok(()));
        // Another settlement's doesn't count.
        w.settlements.push(tgmap::Settlement {
            id: 61,
            template: Id::from("settlement"),
            owner: 1,
            centre: 70,
        });
        assert_eq!(recruited(&mut w), Err(Refused::Lacking));
    }

    #[test]
    fn a_company_with_nowhere_to_come_out_is_refunded() {
        let mut w = village();
        let rock = Obstacle {
            position: [20.0, 20.0],
            facing: [0.0, -1.0],
            shape: Shape::Circle { radius: 2.0 },
            soft: Shape::Circle { radius: 2.0 },
        };
        w.scenery.push(rock);
        w.apply(1, recruit("company_pikeman")).unwrap();
        assert!(w.organizations.is_empty());
        assert_eq!(w.stockpile(1), 500.0);
        let failed = Notice::NotCreated {
            player: 1,
            template: Id::from("company_pikeman"),
        };
        assert!(w.notices.contains(&failed));
    }

    #[test]
    fn a_new_companys_members_arrive_one_about_every_ten_seconds_in_supply() {
        let mut w = village();
        (w.rules.supplies).insert(Id::from("village"), supply(30.0, false, 0.0));
        // A company needs no HQ to be in supply.
        let detached = tgmap::Logistics {
            detached: true,
            ..logistics(None)
        };
        (w.rules.logistics).insert(Id::from("company_pikeman"), detached);
        let company = (w.rules.raisable.get_mut(&Id::from("company_pikeman"))).unwrap();
        for m in &mut company.members {
            m.fighter = Some(soldier(Vec::new()));
        }
        w.apply(1, recruit("company_pikeman")).unwrap();
        let arrived = |w: &World| {
            let r = &w.organizations[0];
            r.units.iter().filter(|&&u| w.units[u].alive()).count()
        };
        run(&mut w, 2.0);
        assert!(w.organizations[0].in_supply);
        assert_eq!(arrived(&w), 1);
        // At 0.1 of its health a second, each comes ten seconds after the
        // one before.
        run(&mut w, 9.5);
        assert_eq!(arrived(&w), 2);
        run(&mut w, 10.0);
        assert_eq!(arrived(&w), 3);
    }
}
