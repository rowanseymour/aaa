//! Players' orders: whether each may be given, and carrying it out.

use aaa_map::Id;

use crate::packing::unpack_facing;
use crate::{
    Building, Command, Commission, Enemy, NotYours, Notice, Order, Refused, TICK, Work, World,
    normalised, unpack_site,
};

impl World {
    /// Whether `player` may give `regiment` orders.
    pub fn commands(&self, player: u32, regiment: usize) -> bool {
        self.regiments
            .get(regiment)
            .is_some_and(|r| r.owner == player)
            && !self.destroyed(regiment)
    }

    /// Whether `player` may give `command` now: what [`World::apply`]
    /// would refuse it for, without carrying it out.
    pub fn check(&self, player: u32, command: &Command) -> Result<(), Refused> {
        match *command {
            Command::Move { regiment, .. }
            | Command::Guard { regiment, .. }
            | Command::Attack { regiment, .. }
            | Command::Run { regiment, .. }
            | Command::Stop { regiment }
            | Command::Formation { regiment, .. } => Ok(self.may_command(player, regiment)?),
            Command::Pursue { regiment, enemy } => self.may_pursue(player, regiment, enemy),
            Command::GuardThing { regiment, thing } => self.may_guard(player, regiment, thing),
            Command::Unpack {
                regiment,
                at,
                facing,
            } => self.may_unpack(player, regiment, at, facing).map(drop),
            Command::Pack { building } => self.may_pack(player, building).map(drop),
            Command::Commission {
                building,
                ref commission,
            } => match &commission.work {
                Work::Raise(regiment) => self.may_commission(player, building, regiment),
                Work::Research(_) => Err(Refused::Unable),
            },
            Command::Research {
                building,
                ref technology,
            } => self.may_research(player, building, technology).map(drop),
            Command::CancelCommission {
                building,
                commission,
            } => self.may_cancel(player, building, commission).map(drop),
            Command::Rally { building, .. } => self.may_rally(player, building),
            Command::Attach { regiment } => self.may_attach(player, regiment).map(drop),
            Command::Detach { regiment } => self.may_detach(player, regiment),
            Command::Disband { regiment } => self.may_disband(player, regiment),
            Command::Sell { building } => self.may_sell(player, building),
        }
    }

    /// Carries out `command` from `player`, unless it's refused.
    pub fn apply(&mut self, player: u32, command: Command) -> Result<(), Refused> {
        match command {
            Command::Move { regiment, to } => self.order_move(player, regiment, to)?,
            Command::Attack { regiment, to } => self.order_attack(player, regiment, to)?,
            Command::Run { regiment, to } => self.order_run(player, regiment, to)?,
            Command::Pursue { regiment, enemy } => self.order_pursue(player, regiment, enemy)?,
            Command::Stop { regiment } => self.order_stop(player, regiment)?,
            Command::Guard { regiment, to } => self.order_guard(player, regiment, to)?,
            Command::GuardThing { regiment, thing } => {
                self.order_guard_thing(player, regiment, thing)?
            }
            Command::Unpack {
                regiment,
                at,
                facing,
            } => {
                self.order_unpack(player, regiment, at, facing)?;
            }
            Command::Formation {
                regiment,
                formation,
            } => self.set_formation(player, regiment, formation)?,
            Command::Pack { building } => self.order_pack(player, building)?,
            Command::Commission {
                building,
                commission,
            } => self.commission(player, building, *commission)?,
            Command::Research {
                building,
                technology,
            } => self.research(player, building, &technology)?,
            Command::CancelCommission {
                building,
                commission,
            } => self.cancel_commission(player, building, commission)?,
            Command::Rally { building, at } => self.set_rally(player, building, at)?,
            Command::Attach { regiment } => {
                let hq = self.may_attach(player, regiment)?;
                let left = self.regiments[regiment].hq.replace(hq);
                let left = left.map(|hq| Notice::Detached {
                    player,
                    hq,
                    regiment,
                });
                self.notices.extend(left);
                let joined = Notice::Attached {
                    player,
                    hq,
                    regiment,
                };
                self.notices.push(joined);
            }
            Command::Detach { regiment } => {
                self.may_detach(player, regiment)?;
                let left = self.regiments[regiment].hq.take();
                let left = left.map(|hq| Notice::Detached {
                    player,
                    hq,
                    regiment,
                });
                self.notices.extend(left);
            }
            Command::Disband { regiment } => {
                self.may_disband(player, regiment)?;
                self.disband(regiment);
            }
            Command::Sell { building } => {
                self.may_sell(player, building)?;
                self.sell(building);
            }
        }
        Ok(())
    }

    pub(crate) fn may_command(&self, player: u32, regiment: usize) -> Result<(), NotYours> {
        match self.commands(player, regiment) {
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

    fn may_pursue(&self, player: u32, regiment: usize, enemy: usize) -> Result<(), Refused> {
        self.may_command(player, regiment)?;
        if !self.armed(regiment)
            || enemy >= self.regiments.len()
            || self.destroyed(enemy)
            || !self.hostile(regiment, Enemy::Regiment(enemy))
        {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    /// Whether any of a regiment's members has something to fire.
    pub(crate) fn armed(&self, regiment: usize) -> bool {
        self.survivors(regiment).any(|u| {
            self.units[u]
                .fighter
                .as_ref()
                .is_some_and(|f| !f.weapons.is_empty())
        })
    }

    fn may_guard(&self, player: u32, regiment: usize, thing: Enemy) -> Result<(), Refused> {
        self.may_command(player, regiment)?;
        let there = match thing {
            Enemy::Regiment(g) => g != regiment && g < self.regiments.len(),
            Enemy::Building(b) => b < self.buildings.len(),
        };
        match there && !self.left(thing) {
            true => Ok(()),
            false => Err(Refused::Unable),
        }
    }

    /// Whether something a regiment could guard is no longer there to.
    pub(crate) fn left(&self, thing: Enemy) -> bool {
        match thing {
            Enemy::Regiment(g) => self.destroyed(g),
            Enemy::Building(b) => self.buildings[b].gone(),
        }
    }

    /// Where the regiment stops to unpack, relative to where it unpacks,
    /// if it can there.
    fn may_unpack(
        &self,
        player: u32,
        regiment: usize,
        at: [f32; 2],
        facing: [f32; 2],
    ) -> Result<[f32; 2], Refused> {
        self.may_command(player, regiment)?;
        let Some(unpacks) = &self.regiments[regiment].unpacks else {
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
        let Some(packs) = self.packing.get(&b.template) else {
            return Err(Refused::Unable);
        };
        if b.busy() || b.health <= packs.health_min * b.fighter.health {
            return Err(Refused::Unable);
        }
        Ok((packs.seconds / TICK).round() as u64)
    }

    fn may_commission(
        &self,
        player: u32,
        building: usize,
        regiment: &aaa_map::Regiment,
    ) -> Result<(), Refused> {
        let b = self.standing_for(player, building)?;
        if b.deploy.is_none() || b.busy() {
            return Err(Refused::Unable);
        }
        if !self.lacks(player, building, regiment).is_empty() {
            return Err(Refused::Lacking);
        }
        // What an HQ raises it takes on, queued or out.
        let hq = self.hq_of(building);
        let hq = hq.and_then(|id| self.hqs().into_iter().find(|h| h.id == id));
        if hq.is_some_and(|h| self.free_places(&h) == 0) {
            return Err(Refused::Full);
        }
        if self.cost(regiment) > self.stockpile(player) {
            return Err(Refused::Unaffordable);
        }
        Ok(())
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
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.advance(player, regiment, target, Order::Move, None)
    }

    fn order_attack(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.advance(player, regiment, target, Order::Attack, None)
    }

    pub(crate) fn order_pursue(
        &mut self,
        player: u32,
        regiment: usize,
        enemy: usize,
    ) -> Result<(), Refused> {
        self.may_pursue(player, regiment, enemy)?;
        let r = &mut self.regiments[regiment];
        r.order = Order::Pursue(enemy);
        r.guard = None;
        r.unpack_at = None;
        r.rally = None;
        r.engaged.clear();
        r.resume = None;
        self.chase(regiment, enemy, true);
        Ok(())
    }

    fn advance(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
        order: Order,
        unpack_at: Option<([f32; 2], [f32; 2])>,
    ) -> Result<(), NotYours> {
        self.may_command(player, regiment)?;
        let r = &mut self.regiments[regiment];
        r.order = order;
        r.guard = None;
        r.unpack_at = unpack_at;
        r.unpack_again = None;
        r.rally = None;
        let from = r.position;
        let facing = normalised([target[0] - from[0], target[1] - from[1]]).unwrap_or(r.facing);
        match r.engaged.is_empty() {
            true => self.send(regiment, target, facing),
            false => r.resume = Some((target, facing)),
        }
        Ok(())
    }

    fn order_run(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.may_command(player, regiment)?;
        let r = &mut self.regiments[regiment];
        r.order = Order::Run;
        r.guard = None;
        r.unpack_at = None;
        r.rally = None;
        r.engaged.clear();
        r.resume = None;
        let from = r.position;
        let facing = normalised([target[0] - from[0], target[1] - from[1]]).unwrap_or(r.facing);
        self.send(regiment, target, facing);
        Ok(())
    }

    pub(crate) fn order_stop(&mut self, player: u32, regiment: usize) -> Result<(), NotYours> {
        self.may_command(player, regiment)?;
        let here = self.centre(regiment);
        let r = &mut self.regiments[regiment];
        r.order = Order::Move;
        r.guard = None;
        r.resume = None;
        r.unpack_at = None;
        r.rally = None;
        if r.engaged.is_empty() {
            let facing = r.facing;
            self.send(regiment, here, facing);
        }
        Ok(())
    }

    fn order_guard(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        self.may_command(player, regiment)?;
        let kept = self.regiments[regiment].guard;
        if kept.is_some_and(|g| g.of.is_none() && g.post == target) {
            return Ok(());
        }
        let from = self.regiments[regiment].position;
        let facing = normalised([target[0] - from[0], target[1] - from[1]]);
        self.post(regiment, None, target, facing);
        Ok(())
    }

    fn order_guard_thing(
        &mut self,
        player: u32,
        regiment: usize,
        thing: Enemy,
    ) -> Result<(), Refused> {
        self.may_guard(player, regiment, thing)?;
        if self.regiments[regiment].guard.and_then(|g| g.of) == Some(thing) {
            return Ok(());
        }
        let (post, facing) = self.beside(regiment, thing);
        self.post(regiment, Some(thing), post, Some(facing));
        Ok(())
    }

    pub(crate) fn order_unpack(
        &mut self,
        player: u32,
        regiment: usize,
        at: [f32; 2],
        facing: [f32; 2],
    ) -> Result<[f32; 2], Refused> {
        let offset = self.may_unpack(player, regiment, at, facing)?;
        let at = unpack_site(at);
        let stop = [at[0] + offset[0], at[1] + offset[1]];
        let unpack_at = Some((at, unpack_facing(facing)));
        self.advance(player, regiment, stop, Order::Move, unpack_at)?;
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
        mut commission: Commission,
    ) -> Result<(), Refused> {
        let Work::Raise(regiment) = &commission.work else {
            return Err(Refused::Unable);
        };
        self.may_commission(player, building, regiment)?;
        commission.paid = self.cost(regiment);
        commission.held = false;
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

#[cfg(test)]
mod tests {
    use crate::testing::*;

    use aaa_map::{Domain, Pathing};

    use crate::{Command, Commission, Fallen, NotYours, Notice, Refused};

    #[test]
    fn regiments_only_take_orders_from_their_owner() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        assert_eq!(w.order_move(8, 0, [10.0, 20.0]), Err(NotYours));
        assert_eq!(w.set_formation(8, 0, 1), Err(NotYours));
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [9.0, 10.0]));
        assert_eq!(w.regiments[0].formation, 0);
    }

    #[test]
    fn regiments_added_later_take_orders_like_the_first() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        let second = aaa_map::Regiment {
            hq: None,
            id: 4,
            template: "squad".into(),
            owner: 7,
            facing: [0.0, 1.0],
            members: vec![member(5, [20.0, 10.0], 2.0)],
            formations: vec![aaa_map::Formation {
                id: "line".into(),
                offsets: vec![[0.0, 0.0]],
                aggressive: false,
            }],
            formation: 0,
            combat: None,
            operational_range: None,
            control_range: None,
            primary: None,
            unpacks: None,
        };
        let r = w.add_regiment(&second);
        assert_eq!((r, w.units[2].regiment), (1, 1));
        w.apply(
            7,
            Command::Move {
                regiment: r,
                to: [20.0, 20.0],
            },
        )
        .unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[2].position, [20.0, 20.0]));
        assert!(near(w.units[0].position, [9.0, 10.0]));
    }

    #[test]
    fn destroyed_regiments_take_no_more_orders() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.destroy(0);
        assert!(w.destroyed(0));
        assert!(!w.commands(7, 0));
        let lost = Notice::Destroyed {
            owner: w.regiments[0].owner,
            what: Fallen::Regiment(0),
        };
        assert_eq!(w.notices, [lost]);
    }

    #[test]
    fn commands_are_refused_as_checked_without_being_carried_out() {
        let mut w = headquarters();
        let truck = Commission::new(truck(), 1.0);
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
            (1, Command::Stop { regiment: 9 }, Refused::NotYours),
            (1, Command::Pack { building: 9 }, Refused::NotYours),
        ];
        for (player, command, why) in refused {
            assert_eq!(w.check(player, &command), Err(why), "{command:?}");
            assert_eq!(w.apply(player, command.clone()), Err(why), "{command:?}");
        }
        let command = Command::Commission {
            building: 0,
            commission: Box::new(truck),
        };
        assert_eq!(w.check(1, &command), Ok(()));
        // Checking alone changes nothing.
        assert!(w.buildings[0].queue.is_empty());
        w.apply(1, command).unwrap();
        assert_eq!(w.buildings[0].queue.len(), 1);
    }
}
