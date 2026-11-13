//! Getting rid of what a side owns: disbanding regiments and trucks, and
//! selling buildings.

use aaa_map::Id;

use crate::economy::{Fallen, money};
use crate::{Left, Notice, Refused, State, World};

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
        let besieged = b.besieged.is_some() && !self.economy.sell_under_siege;
        if !self.sells(building) || b.busy() || besieged {
            return Err(Refused::Unable);
        }
        Ok(())
    }

    /// Whether a building's template is one that can be sold.
    pub fn sells(&self, building: usize) -> bool {
        let template = &self.buildings[building].template;
        self.sellable.contains(&Id::from(template.as_str()))
    }

    /// The money a building sells for: a share of its own template's cost,
    /// whatever was paid for it, to the nearest whole amount.
    pub fn sale_value(&self, building: usize) -> f32 {
        let cost = self.economy_of(&self.buildings[building].template).cost;
        (cost.get(money(&self.economy)) * self.economy.sell_factor).round()
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
        r.engaged.clear();
        r.guard = None;
        r.resume = None;
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
        let b = &mut self.buildings[building];
        b.state = State::Sold(self.ticks);
        b.besieged = None;
        b.target = None;
        if let Some(o) = b.obstacle.take() {
            self.pathing.unblock(&o);
        }
        self.drop_queue(building);
        if let Some(hq) = hq {
            self.detach_all(hq);
        }
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
