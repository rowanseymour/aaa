//! Bombard orders: an organization shelling a thing or a place with its
//! members' bombarding weapons, from where they stand or as near as each
//! needs to get. The organization itself stays put; about once a second it
//! hands each member that can bombard what to shell, and a member out of
//! reach drives towards it.

use serde::{Deserialize, Serialize};
use tgmap::Weapon;

use crate::combat::{Aimed, Hit};
use crate::hash::{Digest, Hasher, digest};
use crate::{
    ARRIVAL_REACH, Notice, Order, Piece, Projectile, Refused, Seen, State, TICK, Target, World,
    distance, within,
};

/// Seconds between an organization's orders to its bombarding members.
const REORDER: f32 = 1.0;

/// The most seconds a member in reach waits for its weapon before it
/// looks again.
const WAIT: f32 = 1.0;

/// What a bombard order is given on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Shelled {
    /// An organization or building.
    Thing(Piece),
    /// A spot on the ground.
    Place([f32; 2]),
}

/// A bombard order being carried out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bombardment {
    pub target: Shelled,
    /// Where a thing stood as the order was given, never updated, or the
    /// place: where members out of reach drive to.
    pub spot: [f32; 2],
}

impl Digest for Shelled {
    fn digest(&self, h: &mut Hasher) {
        match *self {
            Shelled::Thing(p) => (0u8, p).digest(h),
            Shelled::Place(at) => (1u8, at).digest(h),
        }
    }
}

digest!(Bombardment { target, spot });

/// Why a bombard order ends.
enum Ended {
    /// Its target is dead, or none of its members can shell it.
    Done,
    /// Its target moves and its side can't see it any more.
    LostInFog,
}

impl World {
    /// Whether the unit at `u` can use `weapon` to bombard now.
    fn shells_with(&self, u: usize, weapon: &Weapon) -> bool {
        weapon.bombard && self.has(u, weapon)
    }

    /// Whether the unit at `u` has a bombarding weapon it can use now.
    pub fn can_bombard(&self, u: usize) -> bool {
        let fighter = self.units[u].fighter.as_ref();
        fighter.is_some_and(|f| f.weapons.iter().any(|w| self.shells_with(u, w)))
    }

    /// Whether any of the organization at `r`'s members can bombard, which
    /// gives it Bombard.
    pub fn bombards_at_all(&self, r: usize) -> bool {
        self.survivors(r).any(|u| self.can_bombard(u))
    }

    /// Whether every armed member of the organization at `r` has only
    /// bombarding weapons, at least one of them, which leaves it no
    /// Attack.
    pub fn only_bombards(&self, r: usize) -> bool {
        let fighters = self
            .survivors(r)
            .filter_map(|u| self.units[u].fighter.as_ref());
        let mut armed = fighters.filter(|f| !f.weapons.is_empty()).peekable();
        armed.peek().is_some() && armed.all(|f| f.weapons.iter().all(|w| w.bombard))
    }

    /// What the unit at `u` shells for `b`: of an organization, whichever
    /// of its members is nearest.
    fn shell_aim(&self, u: usize, b: Bombardment) -> Option<Aimed> {
        let from = self.units[u].position;
        match b.target {
            Shelled::Place(at) => Some(Aimed::Ground(at)),
            Shelled::Thing(Piece::Building(x)) => Some(Aimed::Thing(Target::Building(x))),
            Shelled::Thing(Piece::Organization(e)) => (self.survivors(e))
                .min_by(|&a, &c| {
                    let d = |v: usize| distance(from, self.units[v].position);
                    d(a).total_cmp(&d(c))
                })
                .map(|v| Aimed::Thing(Target::Unit(v))),
        }
    }

    /// Whether the unit at `u` could aim `weapon` at `aimed`, however far
    /// it is: only a weapon that hits an area is aimed at the ground.
    fn bears(&self, u: usize, weapon: &Weapon, aimed: Aimed) -> bool {
        match aimed {
            Aimed::Thing(t) => self.fighter(t).is_some() && self.bears_on(u, weapon, t),
            Aimed::Ground(at) => {
                let off = distance(self.units[u].position, at);
                weapon.area > 0.0
                    && !(weapon.min_range > 0.0 && off <= weapon.min_range)
                    && self.in_arc_at(Target::Unit(u), weapon, at)
            }
        }
    }

    /// The unit at `u`'s weapons it can shell `aimed` with, by index, and
    /// whether each is in reach of it: centre to centre, short of its range.
    fn shelling_weapons(&self, u: usize, aimed: Aimed) -> Vec<(usize, bool)> {
        let Some(fighter) = &self.units[u].fighter else {
            return Vec::new();
        };
        let ranges = self.ranges(u);
        let off = distance(self.units[u].position, self.aimed_point(aimed));
        (fighter.weapons.iter().enumerate())
            .filter(|(_, w)| self.shells_with(u, w) && self.bears(u, w, aimed))
            .map(|(w, _)| (w, off < ranges[w]))
            .collect()
    }

    /// Whether some member of the organization at `r` could take a bombard
    /// order on `b`: one that can't move only from where it stands.
    fn bombardable(&self, r: usize, b: Bombardment) -> bool {
        self.survivors(r).any(|u| {
            let moves = self.units[u].speed_max > 0.0;
            self.shell_aim(u, b).is_some_and(|aimed| {
                (self.shelling_weapons(u, aimed).iter()).any(|&(_, near)| moves || near)
            })
        })
    }

    /// Whether some member of the organization at `r` could take a bombard
    /// order on the place `at`.
    pub fn bombards_place(&self, r: usize, at: [f32; 2]) -> bool {
        let target = Shelled::Place(at);
        self.bombardable(r, Bombardment { target, spot: at })
    }

    /// Whether some member of the organization at `r` is already in reach
    /// to shell `target` where it stands.
    pub(crate) fn bombards_from_here(&self, r: usize, target: Shelled) -> bool {
        let b = Bombardment {
            target,
            spot: self.spot(target),
        };
        self.survivors(r).any(|u| {
            self.shell_aim(u, b).is_some_and(|aimed| {
                (self.shelling_weapons(u, aimed).iter()).any(|&(_, near)| near)
            })
        })
    }

    /// Whether `player` may have the organization at `r` bombard `target`.
    pub(crate) fn may_bombard(
        &self,
        player: u32,
        r: usize,
        target: Shelled,
    ) -> Result<(), Refused> {
        self.may_command(player, r)?;
        let alive = match target {
            Shelled::Place(_) => true,
            Shelled::Thing(Piece::Organization(e)) => {
                e < self.organizations.len() && !self.destroyed(e)
            }
            Shelled::Thing(Piece::Building(b)) => {
                b < self.buildings.len() && self.fighter(Target::Building(b)).is_some()
            }
        };
        let spot = alive.then(|| self.spot(target));
        match spot.is_some_and(|spot| self.bombardable(r, Bombardment { target, spot })) {
            true => Ok(()),
            false => Err(Refused::Unable),
        }
    }

    /// Where `target` is now.
    fn spot(&self, target: Shelled) -> [f32; 2] {
        match target {
            Shelled::Place(at) => at,
            Shelled::Thing(Piece::Organization(e)) => self.centre(e),
            Shelled::Thing(Piece::Building(b)) => self.buildings[b].position,
        }
    }

    pub(crate) fn order_bombard(
        &mut self,
        player: u32,
        r: usize,
        target: Shelled,
    ) -> Result<(), Refused> {
        self.may_bombard(player, r, target)?;
        if self.organizations[r]
            .bombard
            .is_some_and(|b| b.target == target)
        {
            return Ok(());
        }
        let spot = self.spot(target);
        let here = self.standing_point(r);
        let organization = &mut self.organizations[r];
        organization.take_order(Order::Move);
        organization.bombard = Some(Bombardment { target, spot });
        organization.bombarding = true;
        // Those that can't bombard go to their places and stand.
        if organization.engaged.is_empty() {
            let facing = organization.facing;
            self.send(r, here, facing);
            self.order_bombarders(r);
        }
        Ok(())
    }

    /// Has each organization carrying out a bombard order, and not caught
    /// up in a fight, order its members about once a second.
    pub(crate) fn keep_bombarding(&mut self) {
        let every = (REORDER / TICK).round() as u64;
        for r in 0..self.organizations.len() {
            let organization = &self.organizations[r];
            if organization.bombard.is_none()
                || !organization.engaged.is_empty()
                || !(self.ticks + r as u64).is_multiple_of(every)
            {
                continue;
            }
            self.order_bombarders(r);
        }
    }

    /// What the organization at `r` shells now for its order `b`: a thing
    /// that doesn't move, or is gone, is shelled where it stood while that
    /// spot is out of its side's sight.
    fn shelled_now(&self, r: usize, b: Bombardment) -> Result<Bombardment, Ended> {
        let owner = self.organizations[r].owner;
        let spot_seen = || self.seen(owner, b.spot) == Seen::Visible;
        let at_spot = Bombardment {
            target: Shelled::Place(b.spot),
            spot: b.spot,
        };
        let t = match b.target {
            Shelled::Place(_) => return Ok(b),
            Shelled::Thing(Piece::Organization(e)) => {
                if self.destroyed(e) {
                    return Err(Ended::Done);
                }
                if !self.survivors(e).any(|u| self.sees_unit(owner, u)) {
                    return Err(Ended::LostInFog);
                }
                return match self.can_shell(r, b) {
                    true => Ok(b),
                    false => Err(Ended::Done),
                };
            }
            Shelled::Thing(Piece::Building(x)) => x,
        };
        let building = &self.buildings[t];
        if matches!(building.state, State::Destroyed(_)) {
            return Err(Ended::Done);
        }
        if building.gone() {
            return match spot_seen() {
                true => Err(Ended::Done),
                false => Ok(at_spot),
            };
        }
        if !self.sees_building(owner, t) {
            return Ok(at_spot);
        }
        match self.can_shell(r, b) {
            true => Ok(b),
            false => Err(Ended::Done),
        }
    }

    /// Whether any member of the organization at `r` has a bombarding
    /// weapon for `b`'s target, wherever it is.
    fn can_shell(&self, r: usize, b: Bombardment) -> bool {
        self.survivors(r).any(|u| {
            let Some(Aimed::Thing(t)) = self.shell_aim(u, b) else {
                return true;
            };
            let s = Target::Unit(u);
            let fighter = self.units[u].fighter.as_ref();
            self.fighter(t).is_some()
                && fighter.is_some_and(|f| {
                    (f.weapons.iter()).any(|w| self.shells_with(u, w) && self.can_target(s, w, t))
                })
        })
    }

    /// Hands what to shell to each member of the organization at `r` that
    /// can bombard, sending those out of reach towards it, or ends the
    /// order.
    fn order_bombarders(&mut self, r: usize) {
        let Some(b) = self.organizations[r].bombard else {
            return;
        };
        let now = match self.shelled_now(r, b) {
            Ok(now) => now,
            Err(why) => return self.end_bombard(r, why),
        };
        for u in self.survivors(r).collect::<Vec<_>>() {
            if !self.can_bombard(u) {
                continue;
            }
            self.units[u].shelling = Some(now);
            if !self.close_on(u, now) {
                // Finding no way there ends it, without a word.
                return self.end_bombard(r, Ended::Done);
            }
        }
    }

    /// Sends the unit at `u`, if `b` is out of its reach, towards `b`'s
    /// spot; one that can't move gives the order up instead. False if it
    /// finds no way there.
    fn close_on(&mut self, u: usize, b: Bombardment) -> bool {
        let unit = &self.units[u];
        let weapons = (self.shell_aim(u, b)).map_or(Vec::new(), |a| self.shelling_weapons(u, a));
        if weapons.is_empty() || (unit.speed_max <= 0.0 && !weapons.iter().any(|&(_, n)| n)) {
            self.units[u].shelling = None;
            return true;
        }
        if weapons.iter().any(|&(_, near)| near) {
            return true;
        }
        // One on its way is left to get there; one stopped short is sent
        // on again.
        if unit.moving() || within(unit.position, b.spot, ARRIVAL_REACH) {
            return true;
        }
        if self.no_route(u, b.spot) {
            return false;
        }
        let way = self.way(u, b.spot);
        let unit = &mut self.units[u];
        unit.take(way, false);
        unit.shelling = Some(b);
        true
    }

    /// Has the unit at `u`, given something to shell, fire its best
    /// bombarding weapon in reach at it, stopping where it is to.
    pub(crate) fn bombard_with(
        &mut self,
        u: usize,
        hits: &mut Vec<Hit>,
        launched: &mut Vec<Projectile>,
    ) {
        let Some(b) = self.units[u].shelling else {
            return;
        };
        let Some(aimed) = self.shell_aim(u, b) else {
            self.units[u].shelling = None;
            return;
        };
        let weapons = self.shelling_weapons(u, aimed);
        if weapons.is_empty() {
            self.units[u].shelling = None;
            return;
        }
        let rank = |w: usize| {
            let ready = self.ready(u, w);
            let score = match aimed {
                Aimed::Thing(t) => {
                    let rating = match ready {
                        true => crate::abilities::Rating::UseNow,
                        false => crate::abilities::Rating::Wait,
                    };
                    self.score(u, w, rating, t)
                }
                Aimed::Ground(_) => {
                    let fighter = self.units[u]
                        .fighter
                        .as_ref()
                        .expect("units that shell fight");
                    fighter.weapons[w].choice.priority
                }
            };
            (ready, score)
        };
        let best = (weapons.iter())
            .filter(|&&(_, near)| near)
            .map(|&(w, _)| (w, rank(w)))
            .max_by(|(_, a), (_, c)| a.0.cmp(&c.0).then(a.1.total_cmp(&c.1)));
        let Some((w, (ready, _))) = best else {
            return;
        };
        if self.units[u].moving() {
            self.units[u].halt();
        }
        self.units[u].target = aimed.thing();
        if !ready {
            let most = (WAIT / TICK).round() as u64;
            self.units[u].busy = self.ticks + self.recharged_in(u, w, most);
            return;
        }
        self.fire(u, w, aimed, hits, launched);
    }

    /// Ends the organization at `r`'s bombard order, telling its side if
    /// it lost its target in fog. Its members aren't stopped: one shelling
    /// a place goes on till it's sent somewhere.
    fn end_bombard(&mut self, r: usize, why: Ended) {
        let organization = &mut self.organizations[r];
        organization.bombard = None;
        organization.bombarding = false;
        if let Ended::LostInFog = why {
            self.notices.push(Notice::BombardTargetLost {
                player: organization.owner,
                organization: r,
            });
        }
        for u in self.organizations[r].units.clone() {
            let unit = &mut self.units[u];
            if unit
                .shelling
                .is_some_and(|s| matches!(s.target, Shelled::Thing(_)))
            {
                unit.shelling = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tgmap::{FogSetting, Id, Weapon};

    use super::*;
    use crate::testing::*;
    use crate::{Command, Piece};

    /// A bombarding weapon reaching `range`, hitting round where it lands.
    fn mortar(range: f32) -> Weapon {
        Weapon {
            bombard: true,
            area: 2.0,
            ..rifle(range, 10.0)
        }
    }

    /// Player 1's mortarman at 10,10, armed with `weapons`, and player 2's
    /// unarmed man at 10,35, beyond the fight either would pick.
    fn shelling(weapons: Vec<Weapon>) -> World {
        let mut w = battle(
            &[1, 2],
            vec![soldier(weapons), soldier(Vec::new())],
            at_war(),
        );
        w.organizations[1].formations[0].aggressive = false;
        let unit = &mut w.units[1];
        (unit.position, unit.previous) = ([10.0, 35.0], [10.0, 35.0]);
        w.organizations[1].position = [10.0, 35.0];
        w
    }

    fn bombard(target: Shelled) -> Command {
        Command::Bombard {
            organization: 0,
            target,
        }
    }

    const ENEMY: Shelled = Shelled::Thing(Piece::Organization(1));

    #[test]
    fn only_organizations_with_a_bombarding_weapon_take_the_order() {
        let mut w = shelling(vec![rifle(30.0, 10.0)]);
        assert_eq!(w.apply(1, bombard(ENEMY)), Err(Refused::Unable));
        let mut w = shelling(vec![rifle(30.0, 10.0), mortar(30.0)]);
        assert_eq!(w.apply(1, bombard(ENEMY)), Ok(()));
        // An ability needing what the unit doesn't hold isn't there to use.
        let mut needy = mortar(30.0);
        needy.choice.requires = vec![Id::from("howitzers")];
        let mut w = shelling(vec![needy]);
        assert_eq!(w.apply(1, bombard(ENEMY)), Err(Refused::Unable));
    }

    #[test]
    fn what_cant_move_takes_only_what_is_in_reach() {
        let mut w = shelling(vec![mortar(10.0)]);
        w.units[0].speed_max = 0.0;
        assert_eq!(w.apply(1, bombard(ENEMY)), Err(Refused::Unable));
        let mut w = shelling(vec![mortar(30.0)]);
        w.units[0].speed_max = 0.0;
        assert_eq!(w.apply(1, bombard(ENEMY)), Ok(()));
    }

    #[test]
    fn the_ground_is_shelled_only_with_what_hits_an_area() {
        let place = Shelled::Place([10.0, 30.0]);
        let mut w = shelling(vec![Weapon {
            area: 0.0,
            ..mortar(30.0)
        }]);
        assert_eq!(w.apply(1, bombard(place)), Err(Refused::Unable));
        let mut w = shelling(vec![mortar(30.0)]);
        assert_eq!(w.apply(1, bombard(place)), Ok(()));
    }

    #[test]
    fn places_out_of_reach_of_what_cant_move_cant_be_bombarded() {
        // The bombard cursor marks a place out of range by this.
        let mut w = shelling(vec![mortar(10.0)]);
        assert!(w.bombards_place(0, [10.0, 30.0]));
        w.units[0].speed_max = 0.0;
        assert!(!w.bombards_place(0, [10.0, 30.0]));
        assert!(w.bombards_place(0, [10.0, 15.0]));
    }

    #[test]
    fn units_in_reach_shell_from_where_they_stand_as_bombardment() {
        let mut w = shelling(vec![mortar(30.0)]);
        w.organizations[0].formations[0].aggressive = false;
        w.apply(1, bombard(ENEMY)).unwrap();
        assert!(w.organizations[0].bombarding);
        assert!(w.organizations[0].aggressive());
        run(&mut w, 1.0);
        assert!(near(w.units[0].position, [10.0, 10.0]));
        assert!(w.units[1].health < 100.0);
        // Bombardment keeps what it hits out of supply rather than drawing
        // it into a fight.
        assert!(!w.organizations[1].bombarded.is_empty());
        assert!(w.organizations[1].engaged.is_empty());
    }

    #[test]
    fn only_shelling_that_does_damage_is_bombardment() {
        let mut w = shelling(vec![Weapon {
            hits: vec![tgmap::WeaponHit {
                targets: tgmap::Targets::default(),
                on_user: false,
                does: tgmap::Does::Morale(2.0),
            }],
            ..mortar(30.0)
        }]);
        w.apply(1, bombard(ENEMY)).unwrap();
        run(&mut w, 1.0);
        assert!(w.units[0].shot.is_some());
        assert!(w.organizations[1].bombarded.is_empty());
    }

    #[test]
    fn fighting_off_an_enemy_that_comes_near_is_no_bombardment() {
        let mut w = shelling(vec![mortar(30.0)]);
        w.apply(1, bombard(ENEMY)).unwrap();
        run(&mut w, 1.0);
        assert!(!w.organizations[1].bombarded.is_empty());
        // It comes near, and its shelling turns to an engagement.
        let unit = &mut w.units[1];
        (unit.position, unit.previous) = ([10.0, 16.0], [10.0, 16.0]);
        w.organizations[1].position = [10.0, 16.0];
        w.organizations[1].bombarded.clear();
        while w.organizations[0].engaged.is_empty() {
            w.tick();
        }
        let health = w.units[1].health;
        run(&mut w, 3.0);
        assert!(w.organizations[0].bombarding && w.units[1].health < health);
        assert!(w.organizations[1].bombarded.is_empty());
    }

    #[test]
    fn units_out_of_reach_drive_towards_where_the_target_stood() {
        let mut w = shelling(vec![mortar(15.0)]);
        w.apply(1, bombard(ENEMY)).unwrap();
        let spot = w.organizations[0].bombard.unwrap().spot;
        assert!(near(spot, [10.0, 35.0]));
        // The target moving off doesn't move the spot.
        w.units[1].position = [20.0, 35.0];
        run(&mut w, 1.0);
        assert!(near(w.organizations[0].bombard.unwrap().spot, spot));
        assert!(w.units[0].position[1] > 10.5);
        // The organization itself stays.
        assert!(near(w.organizations[0].position, [10.0, 10.0]));
        assert_eq!(w.unit_activity(0), "mind_moving");
        run(&mut w, 8.0);
        assert!(w.units[1].health < 100.0);
        assert!(!w.units[0].moving());
        assert_eq!(w.unit_activity(0), "mind_bombard");
    }

    #[test]
    fn shelling_a_place_hits_what_stands_there() {
        let mut w = shelling(vec![mortar(30.0)]);
        w.apply(1, bombard(Shelled::Place([10.0, 34.0]))).unwrap();
        run(&mut w, 1.0);
        assert!(w.units[1].health < 100.0);
        // It never ends by itself.
        w.units[1].position = [30.0, 35.0];
        run(&mut w, 5.0);
        assert!(w.organizations[0].bombarding);
    }

    #[test]
    fn the_order_ends_with_its_target_or_another_order() {
        let mut w = shelling(vec![mortar(30.0)]);
        w.apply(1, bombard(ENEMY)).unwrap();
        w.units[1].health = 1.0;
        run(&mut w, 3.0);
        assert!(!w.units[1].alive());
        assert!(w.organizations[0].bombard.is_none());
        assert!(!w.organizations[0].bombarding);

        let mut w = shelling(vec![mortar(30.0)]);
        w.apply(1, bombard(ENEMY)).unwrap();
        run(&mut w, 0.5);
        let stop = Command::Stop { organization: 0 };
        w.apply(1, stop).unwrap();
        assert!(!w.organizations[0].bombarding);
        assert!(w.units[0].shelling.is_none());
    }

    #[test]
    fn an_order_that_sends_nobody_anywhere_still_ends_their_shelling() {
        let mut w = shelling(vec![mortar(30.0)]);
        w.apply(1, bombard(ENEMY)).unwrap();
        run(&mut w, 0.5);
        assert!(w.units[0].shelling.is_some());
        // Engaged, a stop sends nobody to their places.
        w.organizations[0].engaged = vec![Piece::Organization(1)];
        w.apply(1, Command::Stop { organization: 0 }).unwrap();
        assert!(w.units[0].shelling.is_none());
    }

    #[test]
    fn a_target_that_moves_lost_in_fog_ends_the_order_with_word() {
        let mut w = shelling(vec![mortar(30.0)]);
        w.rules.sights.insert(
            Id::from("squad"),
            tgmap::Sight {
                range: 40.0,
                remote: 0.0,
            },
        );
        w.set_fog(FogSetting::Full);
        w.apply(1, bombard(ENEMY)).unwrap();
        run(&mut w, 1.0);
        assert!(w.organizations[0].bombarding);
        w.rules.sights.insert(
            Id::from("squad"),
            tgmap::Sight {
                range: 5.0,
                remote: 0.0,
            },
        );
        w.notices.clear();
        run(&mut w, 1.5);
        assert!(!w.organizations[0].bombarding);
        let lost = Notice::BombardTargetLost {
            player: 1,
            organization: 0,
        };
        assert_eq!(w.notices, vec![lost]);
    }

    #[test]
    fn a_building_out_of_sight_is_shelled_where_it_stood() {
        let w = shelling(vec![mortar(40.0)]);
        let mut w = with_buildings(w, &[building(9, 2, [30.0, 30.0])]);
        w.rules.sights.insert(
            Id::from("squad"),
            tgmap::Sight {
                range: 5.0,
                remote: 0.0,
            },
        );
        w.set_fog(FogSetting::Full);
        let house = Shelled::Thing(Piece::Building(0));
        // A thing in fog is taken.
        w.apply(1, bombard(house)).unwrap();
        run(&mut w, 1.0);
        assert!(w.organizations[0].bombarding);
        let shelling = w.units[0].shelling.unwrap();
        assert_eq!(shelling.target, Shelled::Place([30.0, 30.0]));
        assert!(w.buildings[0].health < w.buildings[0].fighter.health);
    }
}
