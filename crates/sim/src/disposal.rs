//! Getting rid of what a side owns: disbanding regiments and trucks, and
//! selling buildings.

use crate::economy::money;
use crate::{Fallen, Left, Notice, Order, Refused, State, World};

impl World {
    /// Whether a regiment is a lone unit rather than an organization, as a
    /// depot's truck is.
    pub fn lone(&self, regiment: usize) -> bool {
        let r = &self.regiments[regiment];
        matches!(&r.units[..], [u] if self.units[*u].template == r.template)
    }

    /// Whether `player` may disband `regiment` now. A lone unit can be
    /// disbanded even while it fights; an organization can't while it's
    /// engaged or attached to an HQ.
    pub fn may_disband(&self, player: u32, regiment: usize) -> Result<(), Refused> {
        self.may_command(player, regiment)?;
        let r = &self.regiments[regiment];
        let fighting = !r.engaged.is_empty() || r.hq.is_some();
        if r.unpack_at.is_some() || (!self.lone(regiment) && fighting) {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    /// Whether `player` may sell `building` now.
    pub fn may_sell(&self, player: u32, building: usize) -> Result<(), Refused> {
        let b = self.standing_for(player, building)?;
        let besieged = b.besieged.is_some() && !self.rules.economy.sell_under_siege;
        if !self.sells(building) || b.busy() || besieged {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    /// Whether a building's template is one that can be sold.
    pub fn sells(&self, building: usize) -> bool {
        let template = &self.buildings[building].template;
        self.rules.sellable.contains(template)
    }

    /// The money a building sells for: a share of its own template's cost,
    /// whatever was paid for it, to the nearest whole amount.
    pub fn sale_value(&self, building: usize) -> f32 {
        let cost = self.economy_of(&self.buildings[building].template).cost;
        (cost.get(money(&self.rules.economy)) * self.rules.economy.sell_factor).round()
    }

    /// Takes a regiment's members off the map at once, without their dying.
    /// An organization's owner is told it's destroyed, as are enemies who
    /// see it; a lone unit just goes.
    pub(crate) fn disband(&mut self, regiment: usize) {
        if let Some(hq) = self.packed_hq(regiment) {
            self.detach_all(hq);
        }
        let ticks = self.ticks;
        for u in self.regiments[regiment].units.clone() {
            let unit = &mut self.units[u];
            if unit.alive() {
                unit.left = Some(Left::Disbanded(ticks));
                unit.besieged = None;
                unit.target = None;
                unit.halt();
                unit.speed = 0.0;
            }
        }
        let r = &mut self.regiments[regiment];
        r.take_order(Order::Move);
        r.engaged.clear();
        if !self.lone(regiment) {
            let owner = self.regiments[regiment].owner;
            let what = Fallen::Regiment(regiment);
            self.notices.push(Notice::Destroyed { owner, what });
        }
    }

    /// Pays a building's owner its sale value and takes it off the map
    /// without its being destroyed.
    pub(crate) fn sell(&mut self, building: usize) {
        let hq = self.hq_of(building);
        self.pay_sale(building);
        self.remove(building, State::Sold(self.ticks));
        if let Some(hq) = hq {
            self.detach_all(hq);
        }
    }

    /// Takes a building off the map, leaving it as `state`: its ground
    /// free again, with nothing queued, aimed at or besieging it.
    pub(crate) fn remove(&mut self, building: usize, state: State) {
        let b = &mut self.buildings[building];
        b.state = state;
        b.besieged = None;
        b.target = None;
        if let Some(o) = b.obstacle.take() {
            self.pathing.unblock(&o);
        }
        self.drop_queue(building);
    }

    /// Gives a building's owner what it sells for, and tells them.
    pub(crate) fn pay_sale(&mut self, building: usize) {
        let paid = self.sale_value(building);
        let player = self.buildings[building].owner;
        self.refund(player, paid);
        self.notices.push(Notice::Sold {
            player,
            building,
            paid,
        });
    }
}

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use aaa_map::{Domain, Id, Tile};

    use crate::{Command, Commission, Enemy, Fallen, Notice, Refused, State, World};

    /// [`economy`] with its house, building 0, selling for half its cost
    /// of 301 money and 40 ammo.
    fn market() -> World {
        let mut w = economy();
        w.rules.economies.get_mut(&Id::from("house")).unwrap().cost =
            amounts(&[("money", 301.0), ("ammo", 40.0)]);
        w.rules.sellable.insert(Id::from("house"));
        w.rules.economy.sell_factor = 0.5;
        w
    }

    #[test]
    fn regiments_disband_unless_fighting_or_attached_and_lone_units_go_quietly() {
        let mut w = division();
        let disband = |regiment| Command::Disband { regiment };
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        assert_eq!(w.check(1, &disband(1)), Err(Refused::Unable));
        w.apply(1, Command::Detach { regiment: 1 }).unwrap();
        w.regiments[1].engaged.push(Enemy::Regiment(0));
        assert_eq!(w.check(1, &disband(1)), Err(Refused::Unable));
        // A lone unit goes even while it fights.
        w.regiments[0].engaged.push(Enemy::Regiment(1));
        assert_eq!(w.check(1, &disband(0)), Ok(()));
        assert_eq!(w.check(2, &disband(0)), Err(Refused::NotYours));
        w.regiments[1].engaged.clear();
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        w.apply(1, Command::Detach { regiment: 1 }).unwrap();
        w.notices.clear();
        w.apply(1, disband(1)).unwrap();
        assert!(w.destroyed(1));
        // Gone, not killed, and its owner is told as for any loss.
        let units = &w.regiments[1].units;
        assert!(units.iter().all(|&u| w.units[u].died.is_none()));
        let lost = Notice::Destroyed {
            owner: 1,
            what: Fallen::Regiment(1),
        };
        assert_eq!(w.notices, [lost]);
        assert_eq!(w.check(1, &disband(1)), Err(Refused::NotYours));
        // The HQ's truck takes its attachments with it, telling no one.
        let mut c = raised_company(&mut w, 1);
        c.renumber(80);
        c.place([26.0, 20.0], [0.0, 1.0]);
        let r = w.add_regiment(&c);
        w.apply(1, Command::Attach { regiment: r }).unwrap();
        w.notices.clear();
        w.apply(1, disband(0)).unwrap();
        assert!(w.destroyed(0));
        assert_eq!(w.regiments[r].hq, None);
        assert!(w.notices.is_empty());
    }

    #[test]
    fn trucks_on_their_way_to_unpack_cant_be_disbanded() {
        let mut w = headquarters();
        let mut t = truck();
        t.owner = 1;
        t.renumber(30);
        t.place([30.0, 30.0], [0.0, 1.0]);
        let r = w.add_regiment(&t);
        let unpack = Command::Unpack {
            regiment: r,
            at: [30.0, 40.0],
            facing: NORTH,
        };
        w.apply(1, unpack).unwrap();
        assert_eq!(
            w.check(1, &Command::Disband { regiment: r }),
            Err(Refused::Unable)
        );
    }

    #[test]
    fn buildings_sell_for_a_share_of_their_cost_refunding_their_queue() {
        let mut w = market();
        assert_eq!(w.sale_value(0), 151.0);
        w.apply(
            1,
            Command::Commission {
                building: 0,
                commission: Box::new(Commission::new(truck(), 10.0)),
            },
        )
        .unwrap();
        assert_eq!(w.stockpile(1), 250.0);
        let sell = Command::Sell { building: 0 };
        assert_eq!(w.check(2, &sell), Err(Refused::NotYours));
        w.notices.clear();
        w.apply(1, sell.clone()).unwrap();
        assert_eq!(w.stockpile(1), 500.0 + 151.0);
        assert_eq!(w.buildings[0].state, State::Sold(w.ticks));
        assert!(w.buildings[0].queue.is_empty());
        assert!(w.pathing.passable(Tile::new(20, 20), Domain::Land));
        let sold = Notice::Sold {
            player: 1,
            building: 0,
            paid: 151.0,
        };
        assert_eq!(w.notices, [sold]);
        assert_eq!(w.check(1, &sell), Err(Refused::NotYours));
    }

    #[test]
    fn buildings_sell_only_when_their_template_and_state_allow() {
        let mut w = market();
        let sell = Command::Sell { building: 0 };
        w.buildings[0].besieged = Some(w.ticks + 10);
        assert_eq!(w.check(1, &sell), Ok(()));
        w.rules.economy.sell_under_siege = false;
        assert_eq!(w.check(1, &sell), Err(Refused::Unable));
        w.buildings[0].besieged = None;
        w.buildings[0].packing = Some((0, 10));
        assert_eq!(w.check(1, &sell), Err(Refused::Unable));
        w.buildings[0].packing = None;
        w.rules.sellable.clear();
        assert_eq!(w.check(1, &sell), Err(Refused::Unable));
    }

    #[test]
    fn selling_an_hq_detaches_its_regiments_and_takes_its_regiment_too() {
        let mut w = division();
        w.rules.economies.get_mut(&Id::from("depot")).unwrap().cost = amounts(&[("money", 100.0)]);
        w.rules.sellable.insert(Id::from("depot"));
        w.rules.economy.sell_factor = 0.5;
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        let unpack = Command::Unpack {
            regiment: 0,
            at: [30.0, 40.0],
            facing: NORTH,
        };
        w.apply(1, unpack).unwrap();
        run(&mut w, 11.0);
        let depot = w.regiments[0].unpacked_into.unwrap();
        assert_eq!(w.hq_of(depot), Some(50));
        assert_eq!(w.regiments[1].hq, Some(50));
        let before = w.stockpile(1);
        w.notices.clear();
        w.apply(1, Command::Sell { building: depot }).unwrap();
        assert_eq!(w.stockpile(1), before + 50.0);
        assert_eq!(w.regiments[1].hq, None);
        assert!(w.hqs().is_empty());
        // Its regiment goes with it, telling no one it's destroyed.
        assert!(w.destroyed(0));
        assert_eq!(
            w.check(1, &Command::Disband { regiment: 0 }),
            Err(Refused::NotYours)
        );
        let sold = Notice::Sold {
            player: 1,
            building: depot,
            paid: 50.0,
        };
        assert_eq!(w.notices, [sold]);
    }

    #[test]
    fn sellable_buildings_pay_their_owner_when_destroyed() {
        let mut w = market();
        w.buildings[0].fighter.destructible = true;
        // However it stands, as only its template counts.
        w.rules.economy.sell_under_siege = false;
        w.buildings[0].besieged = Some(w.ticks + 10);
        w.notices.clear();
        w.fall(0, None);
        assert_eq!(w.stockpile(1), 651.0);
        let destroyed = Notice::Destroyed {
            owner: 1,
            what: Fallen::Building(0),
        };
        let sold = Notice::Sold {
            player: 1,
            building: 0,
            paid: 151.0,
        };
        assert_eq!(w.notices, [destroyed, sold]);
    }
}
