//! Things told to use one of their abilities on another, once, as
//! [`Command::Ability`](crate::Command::Ability) tells them.

use tgmap::Id;

use crate::combat::{Aimed, Hit};
use crate::targets::Thing;
use crate::{Projectile, State, Target, World};

/// A use a thing has been told to make, waiting for its turret to point
/// at the target.
#[derive(Debug, Clone, PartialEq)]
pub struct Told {
    pub by: Target,
    /// By index into its fighter's weapons.
    pub weapon: usize,
    pub target: Target,
    /// Where the target last stood, which the shot goes off at if the
    /// target's gone before the turret's there.
    pub at: [f32; 2],
}

impl World {
    /// Tells the thing with the map id `thing` to stop and use the
    /// `[PerformerComponent]` ability whose definition is `ability` on the
    /// thing with the id `target`, unless it can't right now: nothing
    /// moves it closer or waits.
    pub(crate) fn tell_ability(&mut self, thing: u32, ability: &Id, target: u32) {
        // A lone unit has its organization's id.
        let fighting = |t: Option<Thing>| match t? {
            Thing::Unit(u) => Some(Target::Unit(u)),
            Thing::Building(b) => Some(Target::Building(b)),
            Thing::Organization(r) if self.lone(r) => {
                Some(Target::Unit(self.organizations[r].units[0]))
            }
            Thing::Organization(_) | Thing::Prop(_) => None,
        };
        let refused = |why: &str| log::debug!("{thing} won't use {ability} on {target}: {why}");
        let (Some(s), Some(t)) = (fighting(self.by_id(thing)), fighting(self.by_id(target))) else {
            return refused("no such thing");
        };
        let Some(fighter) = self.fighter(s) else {
            return refused("it's gone");
        };
        let performed = |w: &tgmap::Weapon| w.performed;
        if !fighter.weapons.iter().any(performed) {
            return refused("it performs nothing");
        }
        let named = (fighter.weapons.iter())
            .position(|w| w.performed && w.definition.as_ref() == Some(ability));
        self.stop_thing(s);
        let Some(w) = named else {
            return refused("it hasn't that ability");
        };
        match self.may_use_told(s, w, t) {
            Err(why) => refused(why),
            Ok(()) => {
                self.told.retain(|x| x.by != s);
                let at = self.position(t);
                self.told.push(Told {
                    by: s,
                    weapon: w,
                    target: t,
                    at,
                });
            }
        }
    }

    /// Stops `s` as a trigger's Stop would: a unit's regiment, and
    /// anything a building was told to use.
    fn stop_thing(&mut self, s: Target) {
        self.told.retain(|x| x.by != s);
        if let Target::Unit(u) = s {
            self.stop_by_trigger(self.units[u].organization);
        }
    }

    /// Whether `s` may use its weapon `w` on `t` now: it has it, `t` is
    /// what it fires on, in range and in its turret's arc, and it's
    /// recharged.
    fn may_use_told(&self, s: Target, w: usize, t: Target) -> Result<(), &'static str> {
        let weapon = &self.fighter(s).ok_or("it's gone")?.weapons[w];
        let has = match s {
            Target::Unit(u) => self.has(u, weapon) && self.unit_may_fire(u),
            Target::Building(b) => !self.buildings[b].constructing,
        };
        let checks = [
            (has, "it may not use it"),
            (self.fighter(t).is_some(), "the target's gone"),
            (
                self.can_target(s, weapon, t),
                "it doesn't fire on the target",
            ),
            (self.can_hit(s, weapon, t), "the target's out of range"),
            (
                self.in_arc(s, weapon, t),
                "the target's out of its turret's arc",
            ),
            (self.told_ready(s, w), "it's recharging"),
            (self.can_pay(s, weapon), "it can't pay for it"),
        ];
        match checks.into_iter().find(|(ok, _)| !ok) {
            Some((_, why)) => Err(why),
            None => Ok(()),
        }
    }

    /// Whether the unit at `u` may fire at all: nothing does while its
    /// regiment routs or while it's coming down.
    fn unit_may_fire(&self, u: usize) -> bool {
        let unit = &self.units[u];
        !self.organizations[unit.organization].routing() && unit.descent.is_none()
    }

    /// The sortie the plane `s` is flying, whose mana pays its weapons.
    fn sortie_of(&self, s: Target) -> Option<usize> {
        let Target::Unit(u) = s else {
            return None;
        };
        (self.sorties.iter()).position(|x| x.plane == Some(u) && !x.stage.over())
    }

    /// Whether `s` can pay for a use of `weapon`: only a plane on a sortie
    /// pays, from the sortie's mana.
    fn can_pay(&self, s: Target, weapon: &tgmap::Weapon) -> bool {
        self.sortie_of(s)
            .is_none_or(|x| self.sorties[x].mana >= weapon.mana_cost)
    }

    /// Whether `s`'s weapon `w` has recharged.
    fn told_ready(&self, s: Target, w: usize) -> bool {
        let (fired_ago, weapon) = match s {
            Target::Unit(u) => {
                let unit = &self.units[u];
                let fighter = unit.fighter.as_ref().expect("told units fight");
                (unit.fired_ago[w], &fighter.weapons[w])
            }
            Target::Building(b) => {
                let b = &self.buildings[b];
                (b.fired_ago[w], &b.fighter.weapons[w])
            }
        };
        fired_ago.is_infinite() || fired_ago * self.fire_rate(s) >= weapon.recharge - 1e-4
    }

    /// The turret of building `b` a use it's been told to make holds.
    pub(crate) fn told_turret(&self, b: usize) -> Option<usize> {
        let told = self.told.iter().find(|x| x.by == Target::Building(b))?;
        self.buildings[b].fighter.weapons[told.weapon].aimed_by
    }

    /// Carries on with each use things have been told to make: turning a
    /// building's turret to the target, then the one shot. A shot whose
    /// target is gone goes off at once where it last stood.
    pub(crate) fn told_uses(&mut self, hits: &mut Vec<Hit>, launched: &mut Vec<Projectile>) {
        for mut told in std::mem::take(&mut self.told) {
            let (s, w, t) = (told.by, told.weapon, told.target);
            let usable = match s {
                Target::Unit(u) => self.units[u].alive() && self.unit_may_fire(u),
                Target::Building(b) => {
                    let state = self.buildings[b].state;
                    state == State::Standing || state == State::Disabled
                }
            };
            // A thing that's fired that weapon since it was told has
            // used it.
            if !usable || !self.told_ready(s, w) {
                continue;
            }
            let there = self.fighter(t).is_some();
            if there {
                told.at = self.position(t);
            }
            if let (Target::Building(b), true) = (s, there)
                && !self.turn_told(b, w, told.at)
            {
                self.told.push(told);
                continue;
            }
            let aimed = match there {
                true => Aimed::Thing(t),
                false => Aimed::Ground(told.at),
            };
            let weapon = self.weapon_of(s, w);
            let (bombard, cost) = (weapon.bombard, weapon.mana_cost);
            match self.sortie_of(s) {
                Some(x) if self.sorties[x].mana < cost => continue,
                Some(x) => self.sorties[x].mana -= cost,
                None => {}
            }
            match s {
                Target::Unit(u) => self.fire_as(u, w, aimed, bombard, hits, launched),
                Target::Building(b) => self.building_fire(b, w, aimed, bombard, hits, launched),
            }
        }
    }

    /// Turns the turret building `b`'s weapon `w` is on toward `at`,
    /// returning whether it points there.
    fn turn_told(&mut self, b: usize, w: usize, at: [f32; 2]) -> bool {
        let building = &self.buildings[b];
        let Some(i) = building.fighter.weapons[w].aimed_by else {
            return true;
        };
        let turret = building.fighter.turrets[i].clone();
        let yaw = building.yaw_to(&turret, at);
        let aim = &mut self.buildings[b].turrets[i];
        aim.idle = 0.0;
        yaw.is_none_or(|yaw| aim.turn(&turret, yaw))
    }

    fn weapon_of(&self, s: Target, w: usize) -> &tgmap::Weapon {
        match s {
            Target::Unit(u) => {
                &self.units[u]
                    .fighter
                    .as_ref()
                    .expect("told units fight")
                    .weapons[w]
            }
            Target::Building(b) => &self.buildings[b].fighter.weapons[w],
        }
    }
}

#[cfg(test)]
mod tests;
