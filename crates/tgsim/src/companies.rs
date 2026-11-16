//! Companies recruited at settlements: made at once, at the settlement,
//! with only their captain, the rest coming as resupply brings them.

use tgmap::Id;

use crate::summons::rings_round;
use crate::{NotYours, Notice, Refused, World, bounding_radius, distance};

/// How far apart the rings round where a company comes out are, in world
/// units: the game's search goes out roughly a unit at a time.
const RING_STEP: f32 = 1.0;

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

    /// The company templates the settlement at `settlement` lists, in its
    /// menu's order, whether or not it can recruit them yet.
    pub fn company_list(&self, settlement: usize) -> &[Id] {
        (self.settlements.get(settlement))
            .and_then(|s| self.rules.recruit_lists.get(&s.template))
            .map_or(&[], Vec::as_slice)
    }

    /// What recruiting a company of `template` at the settlement at
    /// `settlement` would recruit for `player`, unless it's refused.
    pub(crate) fn may_recruit(
        &self,
        player: u32,
        settlement: usize,
        template: &Id,
    ) -> Result<&tgmap::Organization, Refused> {
        (self.settlements.get(settlement))
            .filter(|s| s.owner == player)
            .ok_or(NotYours)?;
        let centre = self.centre_of(settlement).ok_or(NotYours)?;
        let listed = self.company_list(settlement).contains(template);
        let organization = (self.rules.raisable.get(template))
            .filter(|_| listed)
            .ok_or(Refused::Unable)?;
        if !self.lacks(player, centre, organization).is_empty() {
            return Err(Refused::Lacking);
        }
        if self.buildings[centre].besieged.is_some() {
            return Err(Refused::Besieged);
        }
        if self.cost(organization) > self.stockpile(player) {
            return Err(Refused::Unaffordable);
        }
        if self.over_limit(player, organization).is_some() {
            return Err(Refused::OverLimit);
        }
        Ok(organization)
    }

    /// Recruits a company of `template` at the settlement at `settlement`
    /// for `player`, paid for at once. One with nowhere to come out is
    /// refunded, and its side told.
    pub(crate) fn recruit(
        &mut self,
        player: u32,
        settlement: usize,
        template: &Id,
    ) -> Result<(), Refused> {
        let mut organization = self.may_recruit(player, settlement, template)?.clone();
        let centre = self.centre_of(settlement).ok_or(NotYours)?;
        let paid = self.cost(&organization);
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
        log::debug!(
            "{} {} recruited at {} {}",
            self.organizations[r].template,
            self.organizations[r].id,
            self.buildings[centre].template,
            self.buildings[centre].id
        );
        self.notices.push(Notice::PutOut {
            player,
            building: centre,
            organization: r,
        });
        Ok(())
    }

    /// Puts `organization` on the map at the centre at `centre`, facing
    /// the way it does, with only its leader there and the rest awaited,
    /// returning its index; none if its leader has nowhere to stand.
    fn come_out(&mut self, centre: usize, mut organization: tgmap::Organization) -> Option<usize> {
        let b = &self.buildings[centre];
        let (at, facing) = (b.position, b.facing);
        let leader = organization.primary.unwrap_or(0);
        let shape = organization.members.get(leader)?.shape;
        // The centre is what it comes out of, so doesn't stand in its
        // leader's way at the start point.
        let start = std::iter::once(at);
        self.first_clear(start, facing, shape, &self.blocking_but(Some(centre)))?;
        let reach = b
            .obstacle
            .as_ref()
            .map_or(0.0, |o| bounding_radius(o.shape))
            + company_radius(&organization);
        let count = (reach / RING_STEP).ceil().max(1.0) as u32;
        let around = rings_round(at, facing, RING_STEP, count);
        // With nowhere clear round it, it's still made at the start point.
        let stand = (self.first_clear(around, facing, shape, &self.blocking())).unwrap_or(at);
        // The leader stands there, the company's position wherever that
        // puts it.
        let offset = (organization.formations.get(organization.formation))
            .and_then(|f| f.offsets.get(leader).copied())
            .unwrap_or([0.0, 0.0]);
        let shift = tgmap::from_local([0.0, 0.0], facing, offset);
        organization.place([stand[0] - shift[0], stand[1] - shift[1]], facing);
        let r = self.add_new_organization(organization);
        // Those that resupply come out as their leader alone, the rest to
        // follow, as from a building's queue.
        let template = &self.organizations[r].template;
        if self.logistics_of(template).is_some_and(|l| l.supply_used) {
            let leader = self.organizations[r].leader.unwrap_or(0);
            for (p, u) in self.organizations[r].units.clone().into_iter().enumerate() {
                let rate = self.rules.resupply_rates.get(&self.units[u].template);
                self.units[u].awaited = p != leader && rate.is_some_and(|&r| r > 0.0);
            }
        }
        Some(r)
    }
}

/// How far a company's members stand from its position, at most, in its
/// starting formation.
fn company_radius(organization: &tgmap::Organization) -> f32 {
    let offsets = (organization.formations.get(organization.formation))
        .map_or(&[][..], |f| f.offsets.as_slice());
    (offsets.iter())
        .map(|&o| distance(o, [0.0, 0.0]))
        .fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use crate::testing::*;

    use tgmap::{Domain, Id, Obstacle, Pathing, Requires, Shape};

    use crate::{Command, Notice, Refused, World, distance};

    /// Player 1's village centre at 20,20 with its settlement, recruiting
    /// a company of two pikemen and a captain for 8 a pikeman, with room
    /// under the village's limit for two companies, and 500 to spend.
    fn village() -> World {
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

    fn recruit(template: &str) -> Command {
        Command::Recruit {
            settlement: 0,
            template: Id::from(template),
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
