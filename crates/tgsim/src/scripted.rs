//! Orders the map's triggers give things, as a player's orders would be
//! given, and the computer control they switch off and on.

use std::collections::BTreeSet;

use crate::targets::Thing;
use crate::{Command, Piece, Shelled, World};

/// What a trigger's order is carried out on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Aim {
    At([f32; 2]),
    On(Thing),
}

/// What the map's triggers have switched computer control off for.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ComputerControl {
    /// Sides whose computer player is to plan and order nothing, by player
    /// id.
    pub(crate) off_for_sides: BTreeSet<u32>,
    /// Things a side's computer player is to leave alone, by their ids.
    pub(crate) off_for_things: BTreeSet<u32>,
}

crate::hash::digest!(ComputerControl {
    off_for_sides,
    off_for_things
});

/// The way a building unpacked by a trigger's order faces, the order
/// carrying no facing of its own.
const UNPACK_FACING: [f32; 2] = [0.0, -1.0];

/// What `actors_lock` has locked, by their ids: regiments, and each of
/// their units apart.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Locks {
    pub(crate) things: BTreeSet<u32>,
}

crate::hash::digest!(Locks { things });

impl World {
    /// Orders `t` to carry out the command with the id `command` on what
    /// `aim` names, unlocking it first. Whether it took the order: what
    /// doesn't offer the command refuses it and carries on.
    pub(crate) fn order_by_trigger(&mut self, t: Thing, command: &str, aim: Aim) -> bool {
        self.unlock(t);
        // Air Attack is an airfield's, a building's, command.
        if let (Thing::Building(b), "air_attack") = (t, command) {
            return self.air_attack_by_trigger(b, aim);
        }
        let Some(organization) = self.orderable(t) else {
            log::debug!("{t:?} takes no {command} order from triggers");
            return false;
        };
        let Some(order) = self.trigger_command(organization, command, aim) else {
            log::debug!("{t:?} refused {command} {aim:?} from triggers");
            return false;
        };
        let player = self.organizations[organization].owner;
        // A trigger's order leaves a regiment in its group.
        match self.give_order(player, order, true) {
            Ok(()) => true,
            Err(e) => {
                log::debug!("{t:?} refused {command} from triggers: {e:?}");
                false
            }
        }
    }

    /// Has the airfield at `b` send an air attack on or at what `aim`
    /// names, as its owner's order. Whether it took it.
    fn air_attack_by_trigger(&mut self, b: usize, aim: Aim) -> bool {
        let (at, on) = match aim {
            Aim::At(at) => (at, None),
            Aim::On(Thing::Building(target)) => (
                self.buildings[target].position,
                Some(Piece::Building(target)),
            ),
            Aim::On(Thing::Prop(_)) => return false,
            // A unit is aimed at as its regiment.
            Aim::On(t) => match self.regiment_of(t) {
                Some(r) => (self.centre(r), Some(Piece::Organization(r))),
                None => return false,
            },
        };
        let player = self.buildings[b].owner;
        let attack = Command::AirAttack {
            building: b,
            at,
            on,
        };
        match self.give_order(player, attack, true) {
            Ok(()) => true,
            Err(e) => {
                log::debug!("building {b} refused air_attack from triggers: {e:?}");
                false
            }
        }
    }

    /// Orders `t` to move to `to` at `pace`, a share of its top speed
    /// then, or at top speed for 0. Whether it took the order.
    pub(crate) fn move_by_trigger(&mut self, t: Thing, to: [f32; 2], pace: f32) -> bool {
        if !self.order_by_trigger(t, "move", Aim::At(to)) {
            return false;
        }
        if let Some(r) = self.orderable(t)
            && pace > 0.0
            && pace < 1.0
        {
            let speed = pace * self.unhurried_speed(r);
            self.organizations[r].ordered_speed = Some(speed);
        }
        true
    }

    /// Orders every thing of `player`'s on `things` that can move to `to`.
    pub(crate) fn move_all_by_trigger(&mut self, player: u32, things: &[Thing], to: [f32; 2]) {
        let mut ordered = BTreeSet::new();
        for &t in things {
            if self.thing_owner(t) != Some(player) {
                continue;
            }
            match self.orderable(t) {
                Some(r) if ordered.insert(r) => {
                    self.move_by_trigger(t, to, 0.0);
                }
                Some(_) => {}
                None => {
                    self.unlock(t);
                    log::debug!("triggers can't move {t:?} out of its regiment");
                }
            }
        }
    }

    /// The organization an order to `t` goes to: a regiment, or a lone unit's
    /// own organization.
    fn orderable(&self, t: Thing) -> Option<usize> {
        let r = match t {
            Thing::Organization(r) => r,
            Thing::Unit(u) => {
                let r = self.units[u].organization;
                self.lone(r).then_some(r)?
            }
            Thing::Building(_) | Thing::Prop(_) => return None,
        };
        (!self.destroyed(r)).then_some(r)
    }

    /// The player's command that `command` on `aim` is, for `organization`.
    fn trigger_command(&self, organization: usize, command: &str, aim: Aim) -> Option<Command> {
        let piece = |t: Thing| match t {
            Thing::Building(b) => Some(Piece::Building(b)),
            Thing::Prop(_) => None,
            _ => self.regiment_of(t).map(Piece::Organization),
        };
        Some(match (command, aim) {
            ("move", Aim::At(to)) => Command::Move { organization, to },
            ("move", Aim::On(t)) => Command::MoveOnto {
                organization,
                piece: piece(t)?,
            },
            ("kill", Aim::At(to)) => Command::Attack { organization, to },
            // As a player's Attack on it: after an enemy regiment, onto an
            // enemy building, and to anything else.
            ("kill", Aim::On(t)) => {
                let piece = piece(t)?;
                let attackable = self.hostile(organization, piece) && self.armed(organization);
                match (piece, attackable) {
                    // What has only bombarding weapons bombards instead.
                    (_, true) if self.only_bombards(organization) => Command::Bombard {
                        organization,
                        target: Shelled::Thing(piece),
                    },
                    (Piece::Organization(enemy), true) => Command::Pursue {
                        organization,
                        enemy,
                    },
                    (Piece::Building(b), true) => Command::Attack {
                        organization,
                        to: self.buildings[b].position,
                    },
                    (_, false) => Command::MoveOnto {
                        organization,
                        piece,
                    },
                }
            }
            ("guard", Aim::At(to)) => Command::Guard { organization, to },
            ("guard", Aim::On(t)) => Command::GuardPiece {
                organization,
                piece: piece(t)?,
            },
            ("bombard", Aim::At(at)) => Command::Bombard {
                organization,
                target: Shelled::Place(at),
            },
            ("bombard", Aim::On(t)) => Command::Bombard {
                organization,
                target: Shelled::Thing(piece(t)?),
            },
            ("unpack", Aim::At(at)) => Command::Unpack {
                organization,
                at,
                facing: UNPACK_FACING,
            },
            _ => return None,
        })
    }

    /// Stops every thing in the world and locks it, regiments with their
    /// units.
    pub(crate) fn lock_all(&mut self) {
        let standing: Vec<usize> = (0..self.organizations.len())
            .filter(|&r| self.organization_on_map(r) && !self.destroyed(r))
            .collect();
        for r in standing {
            self.stop_by_trigger(r);
            self.triggers.locks.things.insert(self.organizations[r].id);
            for &u in &self.organizations[r].units {
                if self.units[u].alive() {
                    self.triggers.locks.things.insert(self.units[u].id);
                }
            }
        }
        let buildings = (self.buildings.iter()).filter(|b| !b.gone()).map(|b| b.id);
        self.triggers.locks.things.extend(buildings);
    }

    /// Unlocks `t`, and a regiment's units with it.
    fn unlock(&mut self, t: Thing) {
        let Some(id) = self.thing_id(t) else {
            return;
        };
        self.triggers.locks.things.remove(&id);
        if let Thing::Organization(r) = t {
            for &u in &self.organizations[r].units {
                self.triggers.locks.things.remove(&self.units[u].id);
            }
        }
    }

    /// Whether the thing with this id is locked, holding back its queued
    /// orders and its units' answer to being hit.
    pub fn locked(&self, id: u32) -> bool {
        self.triggers.locks.things.contains(&id)
    }

    /// Switches computer control of `player`'s side off or back on. Off,
    /// every organization of the side is told to stop, whoever plays it.
    pub(crate) fn set_side_computer_control(&mut self, player: u32, on: bool) {
        let off = &mut self.triggers.computer.off_for_sides;
        match on {
            true => {
                off.remove(&player);
            }
            false => {
                off.insert(player);
                let theirs: Vec<usize> = (0..self.organizations.len())
                    .filter(|&r| self.organizations[r].owner == player && !self.destroyed(r))
                    .collect();
                for r in theirs {
                    self.stop_by_trigger(r);
                }
            }
        }
    }

    /// Switches computer control of `t` off, telling it to stop, or back on.
    pub(crate) fn set_thing_computer_control(&mut self, t: Thing, on: bool) {
        let Some(id) = self.thing_id(t) else {
            return;
        };
        let off = &mut self.triggers.computer.off_for_things;
        match on {
            true => {
                off.remove(&id);
            }
            false => {
                off.insert(id);
                if let Some(r) = self.orderable(t) {
                    self.stop_by_trigger(r);
                }
            }
        }
    }

    /// Whether a computer player playing `player` may plan and give orders.
    pub fn computer_may_play(&self, player: u32) -> bool {
        !self.triggers.computer.off_for_sides.contains(&player)
    }

    /// Whether a computer player may give orders to the organization at
    /// this index into [`World::organizations`].
    pub fn computer_may_order(&self, organization: usize) -> bool {
        let r = &self.organizations[organization];
        self.computer_may_play(r.owner) && !self.triggers.computer.off_for_things.contains(&r.id)
    }

    fn stop_by_trigger(&mut self, organization: usize) {
        let player = self.organizations[organization].owner;
        let stop = Command::Stop { organization };
        if let Err(e) = self.give_order(player, stop, false) {
            log::debug!("organization {organization} wouldn't stop for triggers: {e:?}");
        }
    }

    /// The id `t` has in the map, or was given as it was made.
    fn thing_id(&self, t: Thing) -> Option<u32> {
        Some(match t {
            Thing::Organization(r) => self.organizations[r].id,
            Thing::Unit(u) => self.units[u].id,
            Thing::Building(b) => self.buildings[b].id,
            Thing::Prop(p) => self.props[p].id,
        })
    }
}
