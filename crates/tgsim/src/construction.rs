//! Organizations putting up buildings and repairing them: going to where a
//! building's to stand, starting its site, raising the site to a finished
//! building, mending what's damaged, and calling a site off.

use tgmap::{Domain, Id, Obstacle, Shape};

use crate::economy::money;
use crate::packing::unpack_facing;
use crate::supply::BEAT;
use crate::targets::User;
use crate::triggers::Happening;
use crate::{
    Build, CONTACT, Notice, Order, PURSUIT_DRIFT, PURSUIT_REACH, Piece, Refused, State, TICK,
    Target, World, squared_distance, within,
};

/// How near full health counts as full, against rounding.
const FULL: f32 = 1e-3;

impl World {
    /// The templates the organization at `r` can build, from what its
    /// members list, each once: the first member's in its own order, and
    /// what each later one adds before those.
    pub fn buildable(&self, r: usize) -> Vec<Id> {
        let mut out: Vec<Id> = Vec::new();
        for u in self.survivors(r) {
            let listed = self.units[u].fighter.iter().flat_map(|f| &f.builds);
            let mut fresh: Vec<Id> = Vec::new();
            for t in listed {
                if !out.contains(t) && !fresh.contains(t) {
                    fresh.push(t.clone());
                }
            }
            fresh.append(&mut out);
            out = fresh;
        }
        out
    }

    /// Whether `player` may send `organization` to build one of `template`.
    /// Where isn't checked: aiming tests only the clicked place, and
    /// regiments sent along with it go to their own, which is tested when
    /// they arrive.
    pub(crate) fn may_build(
        &self,
        player: u32,
        organization: usize,
        template: &Id,
    ) -> Result<(), Refused> {
        self.may_command(player, organization)?;
        let offered = self.buildable(organization).contains(template);
        match offered && self.rules.buildable.contains_key(template) {
            true => Ok(()),
            false => Err(Refused::Unable),
        }
    }

    pub(crate) fn order_build(
        &mut self,
        player: u32,
        organization: usize,
        template: Id,
        at: [f32; 2],
        facing: [f32; 2],
    ) -> Result<(), Refused> {
        self.may_build(player, organization, &template)?;
        self.advance(player, organization, at, Order::Move, None)?;
        let facing = self.build_facing(&template, facing);
        self.organizations[organization].build = Some(Build {
            template,
            at,
            facing,
        });
        Ok(())
    }

    /// Whether the unit at `u` can use one of its repairs on `t` now, as
    /// its side sees it, and if so which.
    fn repair_for(&self, u: usize, t: Target) -> Option<usize> {
        let fighter = self.units[u].fighter.as_ref()?;
        let user = User {
            player: self.organizations[self.units[u].organization].owner,
            thing: Some(Target::Unit(u)),
        };
        (fighter.repairs.iter()).position(|rep| self.reaches(user, &rep.targets, t))
    }

    /// Whether `t` is below its most health.
    fn damaged(&self, t: Target) -> bool {
        match t {
            Target::Unit(u) => {
                self.units[u].alive() && self.units[u].health < self.max_health(u) - FULL
            }
            Target::Building(b) => {
                !self.buildings[b].gone()
                    && self.buildings[b].health < self.building_max_health(b) - FULL
            }
        }
    }

    /// What of `piece` the organization at `r` works on: a building
    /// itself, or a regiment's damaged member nearest it that one of its
    /// members can repair; `None` once there's nothing left to work on.
    fn mend_target(&self, r: usize, piece: Piece) -> Option<Target> {
        let workable = |t: Target| {
            self.damaged(t) && self.survivors(r).any(|u| self.repair_for(u, t).is_some())
        };
        match piece {
            Piece::Building(b) => Some(Target::Building(b)).filter(|&t| workable(t)),
            Piece::Organization(g) => {
                let here = self.centre(r);
                (self.survivors(g).map(Target::Unit))
                    .filter(|&t| workable(t))
                    .min_by(|&a, &b| {
                        let d = |t| squared_distance(here, self.position(t));
                        d(a).total_cmp(&d(b))
                    })
            }
        }
    }

    pub(crate) fn may_repair(
        &self,
        player: u32,
        organization: usize,
        piece: Piece,
    ) -> Result<(), Refused> {
        self.may_command(player, organization)?;
        // A site isn't damaged: it's still going up.
        let there = match piece {
            Piece::Organization(g) => g < self.organizations.len() && g != organization,
            Piece::Building(b) => b < self.buildings.len() && !self.buildings[b].constructing,
        };
        match there && self.mend_target(organization, piece).is_some() {
            true => Ok(()),
            false => Err(Refused::Unable),
        }
    }

    pub(crate) fn order_repair(
        &mut self,
        player: u32,
        organization: usize,
        piece: Piece,
    ) -> Result<(), Refused> {
        self.may_repair(player, organization, piece)?;
        let (to, _) = self.near(organization, piece);
        self.advance(player, organization, to, Order::Move, None)?;
        self.organizations[organization].mending = Some(piece);
        Ok(())
    }

    pub(crate) fn may_cancel_construction(
        &self,
        player: u32,
        building: usize,
    ) -> Result<(), Refused> {
        let b = self
            .buildings
            .get(building)
            .filter(|b| b.owner == player && !b.gone())
            .ok_or(Refused::NotYours)?;
        match b.constructing {
            true => Ok(()),
            false => Err(Refused::Unable),
        }
    }

    /// Takes a site off the map without its dying, refunding its
    /// template's whole cost.
    pub(crate) fn cancel_construction(
        &mut self,
        player: u32,
        building: usize,
    ) -> Result<(), Refused> {
        self.may_cancel_construction(player, building)?;
        let cost = self.site_cost(&self.buildings[building].template);
        self.refund(player, cost);
        self.remove(building, State::Sold(self.ticks));
        Ok(())
    }

    /// The way a building of `template` aimed to face `facing` is put up
    /// facing: the default way for one that can't be turned.
    pub fn build_facing(&self, template: &Id, facing: [f32; 2]) -> [f32; 2] {
        match self.rules.axis_locked.contains(template) {
            true => tgmap::UNPACKED_FACING,
            false => unpack_facing(facing),
        }
    }

    /// Whether `player` could put up a building of `template` at `at`
    /// facing `facing`: all of it on ground the side has explored, with
    /// room round it as far as the side can tell, the units it sees there
    /// counting against it.
    pub fn room_to_build(
        &self,
        player: u32,
        template: &Id,
        at: [f32; 2],
        facing: [f32; 2],
    ) -> bool {
        let footprint = self.footprint(template, at, facing);
        let room = self.build_room(template, at, facing);
        (footprint.zip(room)).is_some_and(|(f, r)| self.room_in_sight(player, &f, &r, true))
    }

    /// Where a building of `template` would stand at `at` facing `facing`.
    fn footprint(&self, template: &Id, at: [f32; 2], facing: [f32; 2]) -> Option<Obstacle> {
        let site = self.rules.buildable.get(template)?;
        let shape = site.obstacle.as_ref().map(|o| o.shape);
        Some(crate::delivery::spot(at, unpack_facing(facing), shape))
    }

    /// The ground that must be clear for a building of `template` to go up
    /// at `at` facing `facing`: its blocking shape padded all round, so it
    /// keeps apart from what's near. One that blocks nothing needs only its
    /// point.
    fn build_room(&self, template: &Id, at: [f32; 2], facing: [f32; 2]) -> Option<Obstacle> {
        let mut room = self.footprint(template, at, facing)?;
        if room.shape != Shape::Point {
            room.shape = room.shape.padded(self.rules.building_padding);
            room.soft = room.shape;
        }
        Some(room)
    }

    /// The money putting up a building of `template` takes.
    pub fn site_cost(&self, template: &Id) -> f32 {
        self.economy_of(template)
            .cost
            .get(money(&self.rules.economy))
    }

    /// Moves on the organizations ordered to build or repair that aren't
    /// fighting: each starts its site once the place is within its control
    /// zone, and closes in on what it's working on, giving up once there's
    /// nothing left to do there. Its members do the work as they act.
    pub(crate) fn construct(&mut self) {
        for r in 0..self.organizations.len() {
            let organization = &self.organizations[r];
            if organization.build.is_none() && organization.mending.is_none() {
                continue;
            }
            if self.destroyed(r) || organization.routing() {
                let organization = &mut self.organizations[r];
                (organization.build, organization.mending) = (None, None);
                continue;
            }
            if !organization.engaged.is_empty() {
                continue;
            }
            if let Some(build) = organization.build.clone() {
                let reach = organization.control(&self.rules.guard_rules);
                let zone = self.pathing.zone(self.centre(r), reach);
                if zone.holds(build.at) {
                    self.start_site(r, build);
                } else if !self.underway(r) {
                    // As near as it can get, and short of it.
                    self.organizations[r].build = None;
                }
                continue;
            }
            let Some(piece) = organization.mending else {
                continue;
            };
            if !self.still_mending(r, piece) {
                self.organizations[r].mending = None;
                continue;
            }
            let (here, there) = (self.centre(r), self.middle(piece));
            let reach = PURSUIT_REACH * organization.control(&self.rules.guard_rules);
            if within(here, there, reach) || !(self.ticks + r as u64).is_multiple_of(BEAT) {
                continue;
            }
            // What's still flying in goes where its delivery takes it.
            if self.flown(r) {
                continue;
            }
            let (to, facing) = self.near(r, piece);
            let off = squared_distance(self.organizations[r].position, to);
            if self.underway(r) && off <= PURSUIT_DRIFT * squared_distance(here, to) {
                continue;
            }
            self.send(r, to, facing);
            // Sent off and going nowhere, it can't get any nearer.
            if !self.underway(r) {
                self.organizations[r].mending = None;
            }
        }
    }

    /// Whether the organization at `r` still has work on `piece`: it's
    /// there, something of it is damaged, and a finished building isn't
    /// repaired while its side is short of its upkeep.
    fn still_mending(&self, r: usize, piece: Piece) -> bool {
        if self.left(piece) || self.mend_target(r, piece).is_none() {
            return false;
        }
        let site = matches!(piece, Piece::Building(b) if self.buildings[b].constructing);
        site || self.short(self.organizations[r].owner).is_none()
    }

    /// Puts up the site of what the organization at `r` was sent to build,
    /// paying for it, and sets it to raising it; or tells its owner why it
    /// can't.
    fn start_site(&mut self, r: usize, build: Build) {
        self.organizations[r].build = None;
        let template = build.template;
        let lists =
            |u: &usize| (self.units[*u].fighter.iter()).any(|f| f.builds.contains(&template));
        if !self.survivors(r).any(|u| lists(&u)) {
            return;
        }
        let player = self.organizations[r].owner;
        // What the map bars is offered all the same, and refused silently.
        if self.barred(player, &template) {
            return;
        }
        let cost = self.site_cost(&template);
        if cost > self.stockpile(player) {
            self.notices.push(Notice::CannotAfford {
                player,
                organization: r,
                template,
            });
            return;
        }
        // Now everything there counts against it, seen or not.
        let room = self.build_room(&template, build.at, build.facing);
        let blocking = self.blocking();
        let clear = |room: &Obstacle| self.pathing.room_for(room, Domain::Land, &blocking);
        let site = (room.filter(clear)).and_then(|_| self.rules.buildable.get(&template).cloned());
        let Some(mut site) = site else {
            self.notices.push(Notice::NotBuilt {
                player,
                organization: r,
                template,
            });
            return;
        };
        site.id = self.take_ids(1);
        site.owner = player;
        site.position = build.at;
        site.facing = build.facing;
        site.health = 1.0 / site.fighter.health.max(1.0);
        if let Some(o) = &mut site.obstacle {
            o.position = build.at;
            o.facing = build.facing;
            self.pathing.block(o);
            self.clear_under(o);
        }
        let b = self.buildings.len();
        self.push_buildings(std::slice::from_ref(&site), true);
        self.buildings[b].constructing = true;
        self.empty_hangar(b);
        self.happen(Happening::Begun {
            player,
            template: template.clone(),
        });
        let left = self.stockpile(player) - cost;
        self.set_stockpile(player, left);
        log::debug!(
            "{} {} started {template}",
            self.organizations[r].template,
            self.organizations[r].id
        );
        let site = Piece::Building(b);
        self.organizations[r].mending = Some(site);
        // It was headed for where the site now stands.
        let (to, facing) = self.near(r, site);
        self.send(r, to, facing);
    }

    /// Has the unit at `u`, with no enemy to take on, work on what its
    /// organization is raising or repairing once the organization's near
    /// it: going within reach of it and using its repair on it each time
    /// that's recharged, or, with none for it, going up to it. Returns
    /// whether it did.
    pub(crate) fn mend(&mut self, u: usize) -> bool {
        let r = self.units[u].organization;
        let organization = &self.organizations[r];
        let Some(piece) = organization
            .mending
            .filter(|_| organization.engaged.is_empty())
        else {
            return false;
        };
        let reach = PURSUIT_REACH * organization.control(&self.rules.guard_rules);
        if !within(self.centre(r), self.middle(piece), reach) {
            return false;
        }
        let Some(t) = self.mend_target(r, piece) else {
            return false;
        };
        let s = Target::Unit(u);
        let Some(i) = self.repair_for(u, t) else {
            let unit = &mut self.units[u];
            unit.target = Some(t);
            unit.closing = Some(CONTACT);
            return true;
        };
        let repair = self.units[u]
            .fighter
            .as_ref()
            .expect("units that repair can fight")
            .repairs[i]
            .clone();
        if self.gap(s, t).is_some_and(|gap| gap > repair.range) {
            let unit = &mut self.units[u];
            unit.target = Some(t);
            unit.closing = Some(repair.range);
            return true;
        }
        let rate = self.fire_rate(s);
        let unit = &mut self.units[u];
        unit.target = Some(t);
        if unit.closing.take().is_some() {
            unit.halt();
        }
        let ago = unit.mended_ago[i];
        if !ago.is_infinite() && ago * rate < repair.recharge - 1e-4 {
            return true;
        }
        unit.mended_ago[i] = 0.0;
        unit.mended = Some((self.ticks, i));
        unit.busy = self.ticks + (repair.lasts / TICK).round() as u64;
        self.play_once(u, repair.animation.as_deref());
        let amount = (repair.amount + self.boost(u).heal).max(0.0);
        self.heal(t, amount);
        true
    }

    /// Adds `amount` health to `t`, up to its most, finishing a site that
    /// reaches it.
    fn heal(&mut self, t: Target, amount: f32) {
        match t {
            Target::Unit(u) => {
                let most = self.max_health(u);
                let unit = &mut self.units[u];
                unit.health = (unit.health + amount).min(most);
            }
            Target::Building(b) => {
                let most = self.building_max_health(b);
                let building = &mut self.buildings[b];
                let was = building.health;
                building.health = (was + amount).min(most);
                let full = building.health >= most - FULL;
                let player = building.owner;
                if full && building.constructing {
                    building.constructing = false;
                    let template = building.template.clone();
                    self.happen(Happening::Finished { player, template });
                    self.notices.push(Notice::Constructed {
                        player,
                        building: b,
                    });
                } else if full && was < most - FULL {
                    self.notices.push(Notice::Mended {
                        player,
                        building: b,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::*;

    use tgmap::{Id, Repair, Targets};

    use crate::{Build, Command, Notice, Piece, Refused, State};

    fn ability(required: &[&str], excluded: &[&str]) -> Repair {
        let ids = |l: &[&str]| l.iter().map(|&f| Id::from(f)).collect();
        Repair {
            range: 1.0,
            recharge: 1.5,
            amount: 5.0,
            targets: Targets {
                required: ids(required),
                excluded: ids(excluded),
            },
            animation: None,
            sounds: Vec::new(),
            lasts: 0.0,
        }
    }

    /// Player 1's engineer at 10,10, with 500 money, who builds bunkers
    /// costing 100 that go up as [`building`]s of 100 health, and repairs.
    fn engineers() -> crate::World {
        let mut engineer = soldier(Vec::new());
        engineer.builds = vec![Id::from("bunker")];
        engineer.target = tgmap::Shape::Circle { radius: 1.0 };
        engineer.repairs = vec![
            ability(
                &["constructable", "under_construction", "friend"],
                &["cannot_build"],
            ),
            ability(&["constructable", "friend"], &["under_construction"]),
        ];
        let mut w = battle(&[1, 2], vec![engineer, soldier(Vec::new())], at_war());
        w.units[1].position = [35.0, 35.0];
        w.organizations[1].position = [35.0, 35.0];
        w.rules.economy = tgmap::EconomyRules {
            resources: aa_resources(),
            stockpile_max: 99999.0,
            ..Default::default()
        };
        let bunker = tgmap::Economy {
            cost: amounts(&[("money", 100.0)]),
            ..Default::default()
        };
        w.rules.economies.insert(Id::from("bunker"), bunker);
        let mut site = building(0, 0, [0.0, 0.0]);
        site.template = Id::from("bunker");
        site.fighter.flags.push(Id::from("constructable"));
        w.rules.buildable.insert(Id::from("bunker"), site);
        w.set_stockpile(1, 500.0);
        w
    }

    fn build(at: [f32; 2]) -> Command {
        Command::Build {
            organization: 0,
            template: Id::from("bunker"),
            at,
            facing: [0.0, -1.0],
        }
    }

    #[test]
    fn regiments_offer_what_their_members_build_later_members_first() {
        let mut w = engineers();
        let mut second = w.units[0].clone();
        second.fighter.as_mut().unwrap().builds = vec![Id::from("gun"), Id::from("bunker")];
        w.units.push(second);
        w.organizations[0].units = vec![0, 2];
        assert_eq!(w.buildable(0), [Id::from("gun"), Id::from("bunker")]);
        w.organizations[0].units = vec![2, 0];
        assert_eq!(w.buildable(0), [Id::from("gun"), Id::from("bunker")]);
        w.units[2].fighter.as_mut().unwrap().builds = vec![Id::from("gun")];
        assert_eq!(w.buildable(0), [Id::from("bunker"), Id::from("gun")]);
        assert_eq!(w.check(1, &build([10.0, 20.0])), Ok(()));
        let wall = Command::Build {
            organization: 0,
            template: Id::from("wall"),
            at: [10.0, 20.0],
            facing: [0.0, -1.0],
        };
        assert_eq!(w.check(1, &wall), Err(Refused::Unable));
        assert_eq!(w.check(2, &build([10.0, 20.0])), Err(Refused::NotYours));
    }

    #[test]
    fn a_building_that_cant_be_turned_is_put_up_facing_the_default_way() {
        let mut w = engineers();
        let east = |w: &mut crate::World| {
            let order = Command::Build {
                organization: 0,
                template: Id::from("bunker"),
                at: [10.0, 20.0],
                facing: [1.0, 0.0],
            };
            w.apply(1, order).unwrap();
            w.organizations[0].build.as_ref().unwrap().facing
        };
        assert_eq!(east(&mut w), [1.0, 0.0]);
        w.rules.axis_locked.insert(Id::from("bunker"));
        assert_eq!(east(&mut w), tgmap::UNPACKED_FACING);
    }

    /// Whether player 1 could aim a bunker at `at`.
    fn fits(w: &crate::World, at: [f32; 2]) -> bool {
        w.room_to_build(1, &Id::from("bunker"), at, [0.0, -1.0])
    }

    #[test]
    fn aiming_keeps_the_padding_clear_of_buildings_and_units() {
        let mut w = with_buildings(engineers(), &[building(9, 1, [20.0, 20.0])]);
        for u in &mut w.units {
            u.shape = Some(tgmap::Shape::Circle { radius: 0.5 });
        }
        assert!(!fits(&w, [20.0, 20.0]));
        // Its 6 by 6 clears the house's, but not with 2 all round.
        assert!(!fits(&w, [20.0, 27.0]));
        assert!(fits(&w, [20.0, 29.0]));
        // Nor onto the engineer, or the enemy soldier it sees.
        assert!(!fits(&w, [12.0, 10.0]));
        assert!(!fits(&w, [35.0, 32.0]));
    }

    #[test]
    fn orders_arent_refused_for_the_place_which_is_tested_on_arrival() {
        let mut w = with_buildings(engineers(), &[building(9, 1, [10.0, 20.0])]);
        assert!(!fits(&w, [10.0, 20.0]));
        w.apply(1, build([10.0, 20.0])).unwrap();
        run(&mut w, 1.0);
        assert_eq!(w.buildings.len(), 1);
        assert!(w.notices.contains(&Notice::NotBuilt {
            player: 1,
            organization: 0,
            template: Id::from("bunker"),
        }));
    }

    #[test]
    fn a_site_isnt_started_with_anything_in_its_way_on_arrival() {
        let mut w = engineers();
        w.apply(1, build([10.0, 20.0])).unwrap();
        // The builders themselves, here.
        w.units[0].position = [10.0, 18.0];
        w.units[0].shape = Some(tgmap::Shape::Circle { radius: 0.5 });
        run(&mut w, 1.0);
        assert!(w.buildings.is_empty());
        assert_eq!(w.organizations[0].build, None::<Build>);
        let told = Notice::NotBuilt {
            player: 1,
            organization: 0,
            template: Id::from("bunker"),
        };
        assert!(w.notices.contains(&told));
        assert_eq!(w.stockpile(1), 500.0);
    }

    #[test]
    fn what_the_map_bars_is_dropped_silently_on_arrival() {
        let mut w = engineers();
        let side = w.sides.players.iter_mut().find(|p| p.id == 1).unwrap();
        side.barred = vec![Id::from("BUNKER")];
        // Still offered and ordered.
        assert_eq!(w.buildable(0), [Id::from("bunker")]);
        w.apply(1, build([10.0, 20.0])).unwrap();
        run(&mut w, 1.0);
        assert!(w.buildings.is_empty());
        assert_eq!(w.organizations[0].build, None::<Build>);
        let told = |n: &Notice| matches!(n, Notice::NotBuilt { .. } | Notice::CannotAfford { .. });
        assert!(!w.notices.iter().any(told));
        assert_eq!(w.stockpile(1), 500.0);
    }

    #[test]
    fn a_site_goes_up_once_its_place_is_in_reach_and_builders_raise_it() {
        let mut w = engineers();
        w.apply(1, build([10.0, 40.0])).unwrap();
        assert_eq!(w.stockpile(1), 500.0);
        run(&mut w, 1.0);
        // Out of its control range yet: it's walking there.
        assert_eq!(w.buildings.len(), 0);
        assert!(w.organizations[0].build.is_some());
        let mut ticks = 0;
        while w.buildings.is_empty() && ticks < 400 {
            w.tick();
            ticks += 1;
        }
        let b = &w.buildings[0];
        assert!(b.constructing && b.owner == 1 && b.health < 6.0);
        assert!(near(b.position, [10.0, 40.0]));
        assert_eq!(w.stockpile(1), 400.0);
        assert_eq!(w.organizations[0].mending, Some(Piece::Building(0)));
        assert!(w.buildings[0].busy());
        // 99 health at 5 every 1.5s, after walking up to it.
        run(&mut w, 40.0);
        let b = &w.buildings[0];
        assert!(!b.constructing && b.health == 100.0);
        assert!(w.notices.contains(&Notice::Constructed {
            player: 1,
            building: 0
        }));
        assert_eq!(w.organizations[0].mending, None);
    }

    #[test]
    fn queued_builds_go_up_one_after_another() {
        let mut w = engineers();
        let queued = |command| Command::Queued {
            command: Box::new(command),
        };
        w.apply(1, build([10.0, 20.0])).unwrap();
        w.apply(1, queued(build([20.0, 20.0]))).unwrap();
        assert_eq!(w.organizations[0].queue.len(), 1);
        run_until(&mut w, 20.0, |w| !w.buildings.is_empty());
        // The second waits on the first going up, not just being started.
        run(&mut w, 5.0);
        assert!(w.buildings[0].constructing && w.buildings.len() == 1);
        assert_eq!(w.stockpile(1), 400.0);
        run_until(&mut w, 60.0, |w| w.buildings.len() == 2);
        assert!(!w.buildings[0].constructing);
        assert!(near(w.buildings[1].position, [20.0, 20.0]));
        assert_eq!(w.stockpile(1), 300.0);
        assert!(w.organizations[0].queue.is_empty());
    }

    /// Seconds an engineer regiment, six engineers and a captain whose
    /// repairs recharge at once and take a second, takes over a
    /// 1250-health site, its engineers' recharges multiplied by
    /// `1 + recharge` and their heals added to by `heal`.
    fn raising_time(recharge: f32, heal: f32) -> f32 {
        let mut engineer = soldier(Vec::new());
        engineer.builds = vec![Id::from("bunker")];
        engineer.target = tgmap::Shape::Circle { radius: 1.0 };
        engineer.repairs = vec![Repair {
            recharge: 0.0,
            lasts: 1.0,
            ..ability(&["constructable", "under_construction"], &[])
        }];
        let placed = |i: u32, template: &str, fighter| tgmap::Member {
            template: template.into(),
            fighter: Some(fighter),
            ..member(2 + i, [7.0 + i as f32, 10.0], 2.0)
        };
        let mut members: Vec<_> = (0..6)
            .map(|i| placed(i, "engineer", engineer.clone()))
            .collect();
        members.push(placed(6, "captain", soldier(Vec::new())));
        let regiment = tgmap::Organization {
            id: 1,
            owner: 1,
            members,
            formations: vec![tgmap::Formation {
                id: "line".into(),
                offsets: (0..7).map(|i| [i as f32 - 3.0, 0.0]).collect(),
                aggressive: false,
            }],
            template: "regiment".into(),
            primary: Some(6),
            operational_range: Some(34.0),
            control_range: Some(18.0),
            unpacks: None,
            ..truck()
        };
        let pathing = tgmap::Pathing::uniform(40, tgmap::Domain::Land);
        let rules = engineers().rules().clone();
        let mut w = crate::World::from_organizations_by(at_war(), pathing, rules, &[regiment]);
        w.rules
            .kinds
            .insert(Id::from("engineer"), kind(&["drilled"]));
        let drilled = tgmap::CombatEffect {
            recharge,
            heal,
            ..Default::default()
        };
        w.rules.combat_effects.insert(Id::from("drilled"), drilled);
        w.rules
            .buildable
            .get_mut(&Id::from("bunker"))
            .unwrap()
            .fighter
            .health = 1250.0;
        w.set_stockpile(1, 500.0);
        w.apply(1, build([10.0, 16.0])).unwrap();
        run_until(&mut w, 10.0, |w| !w.buildings.is_empty());
        let started = w.ticks;
        run_until(&mut w, 200.0, |w| !w.buildings[0].constructing);
        (w.ticks - started) as f32 * crate::TICK
    }

    /// Ticks `w` until `done`, failing after `seconds`.
    fn run_until(w: &mut crate::World, seconds: f32, done: impl Fn(&crate::World) -> bool) {
        let end = w.ticks + (seconds / crate::TICK) as u64;
        while !done(w) {
            assert!(w.ticks < end, "not done in {seconds}s");
            w.tick();
        }
    }

    #[test]
    fn six_engineers_raise_a_bunker_at_one_use_a_second_each() {
        // 30 health a second, after closing in; 48 with 3 more a use.
        let plain = raising_time(0.0, 0.0);
        assert!((41.0..45.0).contains(&plain), "{plain}");
        let more = raising_time(0.0, 3.0);
        assert!((25.0..29.0).contains(&more), "{more}");
        // A zero recharge stays zero, however it's multiplied.
        assert_eq!(raising_time(1.0, 0.0), plain);
        assert_eq!(raising_time(-0.5, 0.0), plain);
    }

    #[test]
    fn a_side_that_cant_pay_on_arrival_is_told_and_nothing_goes_up() {
        let mut w = engineers();
        w.set_stockpile(1, 50.0);
        w.apply(1, build([10.0, 20.0])).unwrap();
        run(&mut w, 1.0);
        assert!(w.buildings.is_empty());
        assert_eq!(w.organizations[0].build, None::<Build>);
        let told = Notice::CannotAfford {
            player: 1,
            organization: 0,
            template: Id::from("bunker"),
        };
        assert!(w.notices.contains(&told));
        assert_eq!(w.stockpile(1), 50.0);
    }

    #[test]
    fn sites_cancel_for_their_whole_cost_and_finished_buildings_dont() {
        let mut w = engineers();
        w.apply(1, build([10.0, 20.0])).unwrap();
        run(&mut w, 1.0);
        assert_eq!(w.stockpile(1), 400.0);
        let cancel = Command::CancelConstruction { building: 0 };
        assert_eq!(w.check(2, &cancel), Err(Refused::NotYours));
        w.apply(1, cancel.clone()).unwrap();
        assert!(matches!(w.buildings[0].state, State::Sold(_)));
        assert_eq!(w.stockpile(1), 500.0);
        w.tick();
        assert_eq!(w.organizations[0].mending, None);
        let w = with_buildings(engineers(), &[building(9, 1, [10.0, 20.0])]);
        assert_eq!(w.check(1, &cancel), Err(Refused::Unable));
    }

    #[test]
    fn damaged_buildings_are_repaired_to_full_health() {
        let mut w = with_buildings(engineers(), &[building(9, 1, [10.0, 25.0])]);
        w.buildings[0].fighter.flags.push(Id::from("constructable"));
        let repair = Command::Repair {
            organization: 0,
            piece: Piece::Building(0),
        };
        // Nothing to mend yet.
        assert_eq!(w.check(1, &repair), Err(Refused::Unable));
        w.buildings[0].health = 80.0;
        w.apply(1, repair.clone()).unwrap();
        run(&mut w, 20.0);
        assert_eq!(w.buildings[0].health, 100.0);
        assert!(w.notices.contains(&Notice::Mended {
            player: 1,
            building: 0
        }));
        assert_eq!(w.organizations[0].mending, None);
        // Neither an enemy's nor what can't be repaired.
        w.buildings[0].health = 80.0;
        w.buildings[0].owner = 2;
        assert_eq!(w.check(1, &repair), Err(Refused::Unable));
        w.buildings[0].owner = 1;
        w.buildings[0].fighter.flags.pop();
        assert_eq!(w.check(1, &repair), Err(Refused::Unable));
    }

    #[test]
    fn an_engineer_reads_moving_on_its_way_then_repairing_and_not_engaged() {
        let mut w = with_buildings(engineers(), &[building(9, 1, [10.0, 25.0])]);
        w.buildings[0].fighter.flags.push(Id::from("constructable"));
        w.buildings[0].health = 20.0;
        assert_eq!(w.unit_activity(0), "mind_resting");
        let repair = Command::Repair {
            organization: 0,
            piece: Piece::Building(0),
        };
        w.apply(1, repair).unwrap();
        run(&mut w, 0.5);
        assert_eq!(w.unit_activity(0), "mind_moving");
        let mut ticks = 0;
        while w.unit_activity(0) != "mind_repairing" && ticks < 400 {
            w.tick();
            ticks += 1;
        }
        assert_eq!(w.unit_activity(0), "mind_repairing");
        assert!(!w.unit_states(0).contains(&"mind_engaged"));
    }

    #[test]
    fn a_repair_out_of_reach_is_given_up() {
        let mut w = with_buildings(engineers(), &[building(9, 1, [10.0, 25.0])]);
        w.buildings[0].fighter.flags.push(Id::from("constructable"));
        w.buildings[0].health = 80.0;
        // A wall right across the map between them.
        for x in 0..w.pathing.side as i32 {
            w.pathing.set_blocked(tgmap::Tile::new(x, 6));
        }
        let repair = Command::Repair {
            organization: 0,
            piece: Piece::Building(0),
        };
        w.apply(1, repair).unwrap();
        run(&mut w, 20.0);
        assert_eq!(w.organizations[0].mending, None);
        assert_eq!(w.buildings[0].health, 80.0);
    }

    #[test]
    fn a_repair_is_kept_by_engineers_pulled_off_their_way() {
        let mut w = with_buildings(engineers(), &[building(9, 1, [10.0, 25.0])]);
        w.buildings[0].fighter.flags.push(Id::from("constructable"));
        w.buildings[0].health = 80.0;
        let repair = Command::Repair {
            organization: 0,
            piece: Piece::Building(0),
        };
        w.apply(1, repair).unwrap();
        run(&mut w, 0.5);
        // Pulled well off, as by a fight, while it's still sent where it was.
        let u = w.organizations[0].units[0];
        w.units[u].position = [10.0, 2.0];
        w.units[u].halt();
        run(&mut w, 30.0);
        assert_eq!(w.buildings[0].health, 100.0);
    }

    #[test]
    fn finished_buildings_arent_repaired_while_their_side_is_short() {
        let mut w = with_buildings(engineers(), &[building(9, 1, [10.0, 25.0])]);
        w.buildings[0].fighter.flags.push(Id::from("constructable"));
        w.buildings[0].health = 80.0;
        let repair = Command::Repair {
            organization: 0,
            piece: Piece::Building(0),
        };
        w.apply(1, repair).unwrap();
        w.treasuries.get_mut(&1).unwrap().shortage = Some(10.0);
        w.construct();
        assert_eq!(w.organizations[0].mending, None);
    }

    #[test]
    fn sites_neither_fire_nor_can_be_repaired() {
        let mut w = engineers();
        let mut site = w.rules.buildable[&Id::from("bunker")].clone();
        site.fighter.weapons.push(rifle(40.0, 10.0));
        w.rules.buildable.insert(Id::from("bunker"), site);
        w.apply(1, build([10.0, 20.0])).unwrap();
        w.tick();
        w.buildings[0].awake = true;
        w.buildings[0].noticed.push(Piece::Organization(1));
        run(&mut w, 3.0);
        assert_eq!(w.units[1].health, 100.0);
        let repair = Command::Repair {
            organization: 0,
            piece: Piece::Building(0),
        };
        assert_eq!(w.check(1, &repair), Err(Refused::Unable));
    }
}
