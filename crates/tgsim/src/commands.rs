//! Players' orders: whether each may be given, and carrying it out.

use tgmap::Id;

use crate::packing::unpack_facing;
use crate::{
    Building, Command, Commission, NotYours, Notice, Order, Piece, Refused, TICK, Work, World,
    normalised, unpack_site,
};

impl World {
    /// Whether `player` may give `organization` orders.
    pub fn commands(&self, player: u32, organization: usize) -> bool {
        self.organizations
            .get(organization)
            .is_some_and(|r| r.owner == player)
            && !self.destroyed(organization)
    }

    /// Whether anyone may give `player`'s orders: nobody plays a side
    /// that's resigned.
    fn playable(&self, player: u32) -> Result<(), Refused> {
        match self.resigned(player) {
            true => Err(Refused::NotYours),
            false => Ok(()),
        }
    }

    /// Whether `player` may give `command` now: what [`World::apply`]
    /// would refuse it for, without carrying it out.
    pub fn check(&self, player: u32, command: &Command) -> Result<(), Refused> {
        self.playable(player)?;
        self.heeded(command)?;
        match *command {
            Command::Run { organization, .. } => self.may_run(player, organization),
            Command::Rout { organization } => self.may_rout(player, organization),
            Command::Move { organization, .. }
            | Command::Guard { organization, .. }
            | Command::Attack { organization, .. }
            | Command::Stop { organization }
            | Command::Formation { organization, .. } => {
                Ok(self.may_command(player, organization)?)
            }
            Command::Pursue {
                organization,
                enemy,
            } => self.may_pursue(player, organization, enemy),
            // What can be guarded can be moved onto.
            Command::GuardPiece {
                organization,
                piece,
            }
            | Command::MoveOnto {
                organization,
                piece,
            } => self.may_guard(player, organization, piece),
            Command::Unpack {
                organization,
                at,
                facing,
            } => self.may_unpack(player, organization, at, facing).map(drop),
            Command::Pack { building } => self.may_pack(player, building).map(drop),
            Command::Commission {
                building,
                ref template,
            } => self.may_commission(player, building, template).map(drop),
            Command::Recruit {
                settlement,
                ref template,
                ref hero,
            } => (self.may_recruit(player, settlement, template, hero.as_ref())).map(drop),
            Command::Deliver {
                building,
                ref template,
                to,
            } => self.may_deliver(player, building, template, to),
            Command::Research {
                building,
                ref technology,
            } => self.may_research(player, building, technology).map(drop),
            Command::CancelCommission {
                building,
                commission,
            } => self.may_cancel(player, building, commission).map(drop),
            Command::Rally { building, .. } => self.may_rally(player, building),
            Command::Attach { organization } => self.may_attach(player, organization).map(drop),
            Command::Detach { organization } => self.may_detach(player, organization),
            Command::AttachHero {
                organization,
                ref hero,
            } => self.may_attach_hero(player, organization, hero).map(drop),
            Command::DetachHero { organization } => self.may_detach_hero(player, organization),
            Command::Disband { organization } => self.may_disband(player, organization),
            Command::Sell { building } => self.may_sell(player, building),
            Command::Operate { operation, .. } => self.may_operate(player, operation),
            Command::Build {
                organization,
                ref template,
                at,
                facing,
            } => self.may_build(player, organization, template, (at, facing)),
            Command::Repair {
                organization,
                piece,
            } => self.may_repair(player, organization, piece),
            Command::CancelConstruction { building } => {
                self.may_cancel_construction(player, building)
            }
        }
    }

    /// Carries out `command` from `player`, unless it's refused.
    pub fn apply(&mut self, player: u32, command: Command) -> Result<(), Refused> {
        self.playable(player)?;
        self.heeded(&command)?;
        let steering = steered(&command);
        match command {
            Command::Move { organization, to } => self.order_move(player, organization, to)?,
            Command::Attack { organization, to } => self.order_attack(player, organization, to)?,
            Command::Run { organization, to } => {
                self.may_run(player, organization)?;
                self.flee(organization);
                self.order_run(player, organization, to)?
            }
            Command::Rout { organization } => {
                self.may_rout(player, organization)?;
                self.rout(organization);
            }
            Command::Pursue {
                organization,
                enemy,
            } => self.order_pursue(player, organization, enemy)?,
            Command::Stop { organization } => self.order_stop(player, organization)?,
            Command::Guard { organization, to } => self.order_guard(player, organization, to)?,
            Command::GuardPiece {
                organization,
                piece,
            } => self.order_guard_piece(player, organization, piece)?,
            Command::MoveOnto {
                organization,
                piece,
            } => self.order_move_onto(player, organization, piece)?,
            Command::Unpack {
                organization,
                at,
                facing,
            } => {
                self.order_unpack(player, organization, at, facing)?;
            }
            Command::Formation {
                organization,
                formation,
            } => self.set_formation(player, organization, formation)?,
            Command::Pack { building } => self.order_pack(player, building)?,
            Command::Commission { building, template } => {
                self.commission(player, building, &template, None)?
            }
            Command::Recruit {
                settlement,
                template,
                hero,
            } => self.recruit(player, settlement, &template, hero.as_ref())?,
            Command::AttachHero { organization, hero } => {
                self.attach_hero(player, organization, &hero)?
            }
            Command::DetachHero { organization } => self.detach_hero(player, organization)?,
            Command::Deliver {
                building,
                template,
                to,
            } => {
                self.may_deliver(player, building, &template, to)?;
                self.commission(player, building, &template, Some(to))?
            }
            Command::Research {
                building,
                technology,
            } => self.research(player, building, &technology)?,
            Command::CancelCommission {
                building,
                commission,
            } => self.cancel_commission(player, building, commission)?,
            Command::Rally { building, at } => self.set_rally(player, building, at)?,
            Command::Attach { organization } => {
                let hq = self.may_attach(player, organization)?;
                let left = self.organizations[organization].hq.replace(hq);
                let left = left.map(|hq| Notice::Detached {
                    player,
                    hq,
                    organization,
                });
                self.notices.extend(left);
                let joined = Notice::Attached {
                    player,
                    hq,
                    organization,
                };
                self.notices.push(joined);
            }
            Command::Detach { organization } => {
                self.may_detach(player, organization)?;
                let left = self.organizations[organization].hq.take();
                let left = left.map(|hq| Notice::Detached {
                    player,
                    hq,
                    organization,
                });
                self.notices.extend(left);
            }
            Command::Disband { organization } => {
                self.may_disband(player, organization)?;
                self.disband(organization);
            }
            Command::Sell { building } => {
                self.may_sell(player, building)?;
                self.sell(building);
            }
            Command::Operate { operation, at, on } => {
                self.special_operation(player, operation, at, on)?
            }
            Command::Build {
                organization,
                template,
                at,
                facing,
            } => self.order_build(player, organization, template, at, facing)?,
            Command::Repair {
                organization,
                piece,
            } => self.order_repair(player, organization, piece)?,
            Command::CancelConstruction { building } => {
                self.cancel_construction(player, building)?
            }
        }
        // An order cuts a reform short.
        if let Some(r) = steering {
            self.settle(r);
        }
        Ok(())
    }

    /// Whether the organization `command` is to has the morale to carry
    /// it out: those that move or fight it, or disband it, aren't taken
    /// while it routs or is exhausted.
    fn heeded(&self, command: &Command) -> Result<(), Refused> {
        match steered(command) {
            Some(organization) => self.may_steer(organization),
            None => Ok(()),
        }
    }

    pub(crate) fn may_command(&self, player: u32, organization: usize) -> Result<(), NotYours> {
        match self.commands(player, organization) {
            true => Ok(()),
            false => Err(NotYours),
        }
    }

    /// The building, if `player`'s and standing.
    pub(crate) fn standing_for(&self, player: u32, building: usize) -> Result<&Building, Refused> {
        self.buildings
            .get(building)
            .filter(|b| b.owner == player && b.standing())
            .ok_or(Refused::NotYours)
    }

    fn may_pursue(&self, player: u32, organization: usize, enemy: usize) -> Result<(), Refused> {
        self.may_command(player, organization)?;
        if !self.armed(organization)
            || enemy >= self.organizations.len()
            || self.destroyed(enemy)
            || !self.hostile(organization, Piece::Organization(enemy))
        {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    /// Whether any of an organization's members has something to fire.
    pub(crate) fn armed(&self, organization: usize) -> bool {
        self.survivors(organization).any(|u| {
            self.units[u]
                .fighter
                .as_ref()
                .is_some_and(|f| !f.weapons.is_empty())
        })
    }

    fn may_guard(&self, player: u32, organization: usize, piece: Piece) -> Result<(), Refused> {
        self.may_command(player, organization)?;
        let there = match piece {
            Piece::Organization(g) => g != organization && g < self.organizations.len(),
            Piece::Building(b) => b < self.buildings.len(),
        };
        match there && !self.left(piece) {
            true => Ok(()),
            false => Err(Refused::Unable),
        }
    }

    /// Whether something an organization could guard is no longer there to.
    pub(crate) fn left(&self, piece: Piece) -> bool {
        match piece {
            Piece::Organization(g) => self.destroyed(g),
            Piece::Building(b) => self.buildings[b].gone(),
        }
    }

    /// Where the organization stops to unpack, relative to where it unpacks,
    /// if it can there.
    fn may_unpack(
        &self,
        player: u32,
        organization: usize,
        at: [f32; 2],
        facing: [f32; 2],
    ) -> Result<[f32; 2], Refused> {
        self.may_command(player, organization)?;
        let Some(unpacks) = &self.organizations[organization].unpacks else {
            return Err(Refused::Unable);
        };
        if !self.room_to_unpack(unpacks, unpack_site(at), facing) {
            return Err(Refused::NoRoom);
        }
        Ok(unpacks.offset)
    }

    /// How long the building takes to pack, if it can now.
    fn may_pack(&self, player: u32, building: usize) -> Result<u64, Refused> {
        let b = self.standing_for(player, building)?;
        let Some(packs) = self.rules.packing.get(&b.template) else {
            return Err(Refused::Unable);
        };
        if b.busy() || b.health <= packs.health_min * b.fighter.health * b.health_factor {
            return Err(Refused::Unable);
        }
        Ok((packs.seconds / TICK).round() as u64)
    }

    /// The organization of `template` the building would raise.
    pub(crate) fn may_commission(
        &self,
        player: u32,
        building: usize,
        template: &Id,
    ) -> Result<&tgmap::Organization, Refused> {
        let b = self.standing_for(player, building)?;
        if b.deploy.is_none() || b.busy() {
            return Err(Refused::Unable);
        }
        let listed =
            (self.rules.recruit_lists.get(&b.template)).is_some_and(|l| l.contains(template));
        let organization = (self.rules.raisable.get(template))
            .filter(|_| listed)
            .ok_or(Refused::Unable)?;
        if !self.lacks(player, building, organization).is_empty() {
            return Err(Refused::Lacking);
        }
        // What an HQ raises it takes on, queued or out.
        let hq = self.hq_of(building);
        let hq = hq.and_then(|id| self.hqs().into_iter().find(|h| h.id == id));
        if hq.is_some_and(|h| self.free_places(&h) == 0) {
            return Err(Refused::Full);
        }
        if self.cost(organization) > self.stockpile(player) {
            return Err(Refused::Unaffordable);
        }
        Ok(organization)
    }

    fn may_rally(&self, player: u32, building: usize) -> Result<(), Refused> {
        if self.standing_for(player, building)?.deploy.is_none() {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    /// Where the commission is in the building's queue, if it's there.
    fn may_cancel(&self, player: u32, building: usize, commission: u32) -> Result<usize, Refused> {
        let b = self
            .buildings
            .get(building)
            .filter(|b| b.owner == player)
            .ok_or(Refused::NotYours)?;
        b.queue
            .iter()
            .position(|c| c.id == commission)
            .ok_or(Refused::Unable)
    }

    pub(crate) fn order_move(
        &mut self,
        player: u32,
        organization: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.advance(player, organization, target, Order::Move, None)
    }

    fn order_attack(
        &mut self,
        player: u32,
        organization: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.advance(player, organization, target, Order::Attack, None)
    }

    pub(crate) fn order_pursue(
        &mut self,
        player: u32,
        organization: usize,
        enemy: usize,
    ) -> Result<(), Refused> {
        self.may_pursue(player, organization, enemy)?;
        let r = &mut self.organizations[organization];
        r.take_order(Order::Pursue(enemy));
        r.engaged.clear();
        self.chase(organization, enemy, true);
        Ok(())
    }

    pub(crate) fn advance(
        &mut self,
        player: u32,
        organization: usize,
        target: [f32; 2],
        order: Order,
        unpack_at: Option<([f32; 2], [f32; 2])>,
    ) -> Result<(), NotYours> {
        self.may_command(player, organization)?;
        let r = &mut self.organizations[organization];
        r.take_order(order);
        r.unpack_at = unpack_at;
        let from = r.position;
        let facing = normalised([target[0] - from[0], target[1] - from[1]]).unwrap_or(r.facing);
        match r.engaged.is_empty() {
            true => self.send(organization, target, facing),
            false => r.resume = Some((target, facing)),
        }
        Ok(())
    }

    fn order_run(
        &mut self,
        player: u32,
        organization: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.may_command(player, organization)?;
        let r = &mut self.organizations[organization];
        r.take_order(Order::Run);
        r.engaged.clear();
        let from = r.position;
        let facing = normalised([target[0] - from[0], target[1] - from[1]]).unwrap_or(r.facing);
        self.send(organization, target, facing);
        Ok(())
    }

    pub(crate) fn order_stop(&mut self, player: u32, organization: usize) -> Result<(), NotYours> {
        self.may_command(player, organization)?;
        let here = self.centre(organization);
        let r = &mut self.organizations[organization];
        r.take_order(Order::Move);
        if r.engaged.is_empty() {
            let facing = r.facing;
            self.send(organization, here, facing);
        }
        Ok(())
    }

    fn order_guard(
        &mut self,
        player: u32,
        organization: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.may_command(player, organization)?;
        let kept = self.organizations[organization].guard;
        if kept.is_some_and(|g| g.of.is_none() && g.post == target) {
            return Ok(());
        }
        let from = self.organizations[organization].position;
        let facing = normalised([target[0] - from[0], target[1] - from[1]]);
        self.post(organization, None, target, facing);
        Ok(())
    }

    fn order_move_onto(
        &mut self,
        player: u32,
        organization: usize,
        piece: Piece,
    ) -> Result<(), Refused> {
        self.may_guard(player, organization, piece)?;
        let (to, _) = self.near(organization, piece);
        self.advance(player, organization, to, Order::Move, None)?;
        self.organizations[organization].follow = Some(piece);
        Ok(())
    }

    fn order_guard_piece(
        &mut self,
        player: u32,
        organization: usize,
        piece: Piece,
    ) -> Result<(), Refused> {
        self.may_guard(player, organization, piece)?;
        if self.organizations[organization].guard.and_then(|g| g.of) == Some(piece) {
            return Ok(());
        }
        let (post, facing) = self.beside(organization, piece);
        self.post(organization, Some(piece), post, Some(facing));
        Ok(())
    }

    pub(crate) fn order_unpack(
        &mut self,
        player: u32,
        organization: usize,
        at: [f32; 2],
        facing: [f32; 2],
    ) -> Result<[f32; 2], Refused> {
        let offset = self.may_unpack(player, organization, at, facing)?;
        let at = unpack_site(at);
        let stop = [at[0] + offset[0], at[1] + offset[1]];
        let unpack_at = Some((at, unpack_facing(facing)));
        self.advance(player, organization, stop, Order::Move, unpack_at)?;
        Ok(at)
    }

    pub(crate) fn order_pack(&mut self, player: u32, building: usize) -> Result<(), Refused> {
        let ticks = self.may_pack(player, building)?;
        self.drop_queue(building);
        let b = &mut self.buildings[building];
        b.packing = Some((self.ticks, self.ticks + ticks));
        Ok(())
    }

    pub(crate) fn commission(
        &mut self,
        player: u32,
        building: usize,
        template: &Id,
        to: Option<[f32; 2]>,
    ) -> Result<(), Refused> {
        let organization = self.may_commission(player, building, template)?;
        let paid = self.cost(organization);
        let seconds = self.rules.recruit_times.get(template).copied();
        let work = Work::Raise(Box::new(organization.clone()));
        let mut commission = Commission::of(work, seconds.unwrap_or(0.0));
        commission.paid = paid;
        commission.to = to;
        let left = self.stockpile(player) - commission.paid;
        self.set_stockpile(player, left);
        self.enqueue(building, commission);
        Ok(())
    }

    fn research(&mut self, player: u32, building: usize, technology: &Id) -> Result<(), Refused> {
        let item = self.may_research(player, building, technology)?.clone();
        let (paid, seconds) = (self.research_cost(&item), item.seconds);
        let mut commission = Commission::of(Work::Research(item), seconds);
        commission.paid = paid;
        let left = self.stockpile(player) - commission.paid;
        self.set_stockpile(player, left);
        self.enqueue(building, commission);
        Ok(())
    }

    pub(crate) fn set_rally(
        &mut self,
        player: u32,
        building: usize,
        at: Option<[f32; 2]>,
    ) -> Result<(), Refused> {
        self.may_rally(player, building)?;
        self.buildings[building].rally = at;
        Ok(())
    }

    fn cancel_commission(
        &mut self,
        player: u32,
        building: usize,
        commission: u32,
    ) -> Result<(), Refused> {
        let index = self.may_cancel(player, building, commission)?;
        let b = &mut self.buildings[building];
        let paid = b.queue.remove(index).map_or(0.0, |c| c.paid);
        self.refund(player, paid);
        let b = &mut self.buildings[building];
        if index == 0 {
            b.progress = 0;
            b.paused = None;
        }
        Ok(())
    }
}

/// The organization `command` moves, fights or disbands, if it's one of
/// those orders.
fn steered(command: &Command) -> Option<usize> {
    match *command {
        Command::Move { organization, .. }
        | Command::Attack { organization, .. }
        | Command::Run { organization, .. }
        | Command::Pursue { organization, .. }
        | Command::Guard { organization, .. }
        | Command::GuardPiece { organization, .. }
        | Command::MoveOnto { organization, .. }
        | Command::Stop { organization }
        | Command::Unpack { organization, .. }
        | Command::Build { organization, .. }
        | Command::Repair { organization, .. }
        | Command::Disband { organization } => Some(organization),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::*;

    use tgmap::{Domain, Pathing};

    use crate::{Command, NotYours, Notice, Piece, Refused};

    #[test]
    fn a_new_order_forgets_what_the_organization_was_told_before() {
        let orders = [
            Command::Move {
                organization: 0,
                to: [10.0, 20.0],
            },
            Command::Run {
                organization: 0,
                to: [10.0, 20.0],
            },
            Command::Stop { organization: 0 },
            Command::Guard {
                organization: 0,
                to: [10.0, 20.0],
            },
        ];
        for order in orders {
            let mut w = world(Pathing::uniform(20, Domain::Land));
            w.apply(
                7,
                Command::Guard {
                    organization: 0,
                    to: [5.0, 5.0],
                },
            )
            .unwrap();
            let r = &mut w.organizations[0];
            r.rally = Some([1.0, 1.0]);
            r.unpack_at = Some(([2.0, 2.0], [0.0, 1.0]));
            r.unpack_again = Some(99);
            r.resume = Some(([3.0, 3.0], [0.0, 1.0]));
            w.apply(7, order.clone()).unwrap();
            let r = &w.organizations[0];
            let guarding = matches!(order, Command::Guard { .. });
            assert_eq!(r.guard.is_some(), guarding, "{order:?}");
            assert!(!r.guard.is_some_and(|g| g.post == [5.0, 5.0]), "{order:?}");
            assert_eq!(
                (r.rally, r.unpack_at, r.unpack_again),
                (None, None, None),
                "{order:?}"
            );
            assert_eq!(r.resume, None, "{order:?}");
        }
    }

    #[test]
    fn organizations_only_take_orders_from_their_owner() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        assert_eq!(w.order_move(8, 0, [10.0, 20.0]), Err(NotYours));
        assert_eq!(w.set_formation(8, 0, 1), Err(NotYours));
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [9.0, 10.0]));
        assert_eq!(w.organizations[0].formation, 0);
    }

    #[test]
    fn organizations_added_later_take_orders_like_the_first() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        let second = tgmap::Organization {
            hq: None,
            id: 4,
            template: "squad".into(),
            owner: 7,
            facing: [0.0, 1.0],
            members: vec![member(5, [20.0, 10.0], 2.0)],
            formations: vec![tgmap::Formation {
                id: "line".into(),
                offsets: vec![[0.0, 0.0]],
                aggressive: false,
            }],
            formation: 0,
            combat: None,
            operational_range: None,
            control_range: None,
            primary: None,
            morale: 1.0,
            experience: 0.0,
            efficiency: None,
            unpacks: None,
        };
        let r = w.add_organization(&second);
        assert_eq!((r, w.units[2].organization), (1, 1));
        w.apply(
            7,
            Command::Move {
                organization: r,
                to: [20.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[2].position, [20.0, 20.0]));
        assert!(near(w.units[0].position, [9.0, 10.0]));
    }

    #[test]
    fn destroyed_organizations_take_no_more_orders() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.destroy(0);
        assert!(w.destroyed(0));
        assert!(!w.commands(7, 0));
        let lost = Notice::Destroyed {
            owner: w.organizations[0].owner,
            what: Piece::Organization(0),
        };
        assert_eq!(w.notices, [lost]);
    }

    #[test]
    fn commands_are_refused_as_checked_without_being_carried_out() {
        let mut w = headquarters();
        let refused = [
            (
                2,
                Command::Rally {
                    building: 0,
                    at: None,
                },
                Refused::NotYours,
            ),
            (1, Command::Pack { building: 0 }, Refused::Unable),
            (
                1,
                Command::CancelCommission {
                    building: 0,
                    commission: 0,
                },
                Refused::Unable,
            ),
            // What isn't there is refused, not a panic.
            (1, Command::Stop { organization: 9 }, Refused::NotYours),
            (1, Command::Pack { building: 9 }, Refused::NotYours),
            // Only what the building recruits.
            (1, commission(0, "company"), Refused::Unable),
        ];
        for (player, command, why) in refused {
            assert_eq!(w.check(player, &command), Err(why), "{command:?}");
            assert_eq!(w.apply(player, command.clone()), Err(why), "{command:?}");
        }
        let command = commission(0, "truck");
        assert_eq!(w.check(1, &command), Ok(()));
        // Checking alone changes nothing.
        assert!(w.buildings[0].queue.is_empty());
        w.apply(1, command).unwrap();
        assert_eq!(w.buildings[0].queue.len(), 1);
    }
}
