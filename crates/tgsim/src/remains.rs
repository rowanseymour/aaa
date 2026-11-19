//! What's still on the map: the dead until their remains are taken off,
//! and a building's remains for as long as a side remembers them.

use tgmap::{ANY_DAMAGE, BLEEDING, Death, Id};

use crate::{Left, Seen, State, TICK, World};

/// When a dead thing's remains start to sink and are taken off the map,
/// by tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Remains {
    pub sinking: u64,
    pub gone: u64,
}

impl World {
    /// The ticks the remains of a thing made from `template` lie rotting,
    /// then sink.
    fn remains_ticks(&self, template: &Id) -> [u64; 2] {
        let seconds = self
            .rules
            .remains
            .get(template)
            .copied()
            .unwrap_or([0.0; 2]);
        seconds.map(|s| (s.max(0.0) / TICK).round() as u64)
    }

    /// The remains `template`'s thing leaves from `from`.
    fn remains_from(&self, template: &Id, from: u64) -> Remains {
        let [rot, sink] = self.remains_ticks(template);
        Remains {
            sinking: from + rot,
            gone: from + rot + sink,
        }
    }

    /// The remains of the unit at `u`, once it's died and its death has
    /// played.
    pub fn unit_remains(&self, u: usize) -> Option<Remains> {
        let unit = &self.units[u];
        let died = unit.died.filter(|_| unit.left.is_none())?;
        Some(self.remains_from(&unit.template, died + unit.dying))
    }

    /// The remains of the building at `b`, once it's destroyed.
    pub fn building_remains(&self, b: usize) -> Option<Remains> {
        match self.buildings[b].state {
            State::Destroyed(fell) => Some(self.remains_from(&self.buildings[b].template, fell)),
            _ => None,
        }
    }

    /// The tick the unit at `u` is taken off the map on, if it's to be.
    fn unit_gone_at(&self, u: usize) -> Option<u64> {
        match self.units[u].left {
            Some(Left::Unpacked(t) | Left::Disbanded(t) | Left::Replaced(t)) => Some(t),
            None => self.unit_remains(u).map(|r| r.gone),
        }
    }

    /// The tick the building at `b`'s remains are taken off the map on,
    /// for one destroyed.
    fn remains_gone_at(&self, b: usize) -> Option<u64> {
        self.building_remains(b).map(|r| r.gone)
    }

    /// Whether the unit at `u` is on the map, dying or not: it's come into
    /// being, falling under its parachute or down, and hasn't been taken
    /// off.
    pub(crate) fn unit_on_map(&self, u: usize) -> bool {
        let unit = &self.units[u];
        let made = !unit.awaited || unit.descent.is_some();
        made && self.unit_gone_at(u).is_none_or(|t| self.ticks < t)
    }

    /// Picks the death the unit at `u` plays as it dies, by the last hit
    /// an enemy dealt it, and how long it takes, from the animations of
    /// the stance it's drawn in. If that's one it's leaving, it takes the
    /// stance it dies into as the death starts, playing that stance's
    /// namesake of it, or failing one its first; with no death to start,
    /// it never does.
    pub(crate) fn pick_death(&mut self, u: usize) {
        (self.units[u].death, self.units[u].dying) = (None, 0);
        let (filter, damage) =
            (self.units[u].struck).map_or((BLEEDING, 0.0), |s| (s.filter, s.damage));
        let (into, now) = self.dying_stance(u);
        let unit = &self.units[u];
        let drawn = match now {
            true => into.as_ref(),
            false => unit.stance.as_ref(),
        };
        let set = self.animations_in(u, drawn);
        let deaths = set.map_or(&[][..], |s| s.deaths.as_slice());
        let fits = |d: &&Death| d.filter == ANY_DAMAGE || d.filter == filter;
        let kept: Vec<Death> = match deaths {
            [only] => [only].into_iter().filter(fits).cloned().collect(),
            _ => {
                let met = || deaths.iter().filter(fits).filter(|d| d.threshold <= damage);
                let top = met().map(|d| d.threshold).fold(f32::NEG_INFINITY, f32::max);
                met().filter(|d| d.threshold == top).cloned().collect()
            }
        };
        let death = match kept.as_slice() {
            [] => None,
            [one] => Some(one.clone()),
            _ => {
                let total: f32 = kept.iter().map(|d| d.weight.max(0.0)).sum();
                let mut at = self.chance() * total;
                (kept.iter())
                    .find(|d| {
                        at -= d.weight.max(0.0);
                        at < 0.0
                    })
                    .or(kept.last())
                    .cloned()
            }
        };
        let Some(death) = death else {
            if now {
                self.units[u].stance = into;
            }
            return;
        };
        let played = match !now {
            true => {
                let theirs = self
                    .animations_in(u, into.as_ref())
                    .map(|s| s.deaths.as_slice());
                let theirs = theirs.unwrap_or_default();
                (theirs.iter().find(|d| d.ids == death.ids))
                    .or(theirs.first())
                    .map_or(death.ids.clone(), |d| d.ids.clone())
            }
            false => death.ids.clone(),
        };
        let unit = &mut self.units[u];
        unit.stance = into;
        unit.death = Some(played);
        unit.dying = (death.length.max(0.0) / TICK).round() as u64;
    }

    /// Whether the organization at `r` is on the map: one with no body of
    /// its own leaves as its last member dies, but not as it unpacks.
    pub(crate) fn organization_on_map(&self, r: usize) -> bool {
        self.organizations[r].unpacked_into.is_some() || !self.destroyed(r)
    }

    /// Whether the building at `b` is still there, remains and all, or
    /// remembered by some side under the fog.
    pub(crate) fn building_on_map(&self, b: usize) -> bool {
        let building = &self.buildings[b];
        match building.state {
            State::Standing | State::Disabled => true,
            State::Packed(..) | State::Sold(_) => false,
            State::Destroyed(_) => {
                self.remains_gone_at(b).is_some_and(|t| self.ticks < t)
                    || !building.remembered.is_empty()
            }
        }
    }

    /// Takes off the map the remains of buildings whose time is up, each
    /// side not watching them go remembering them, and forgets those a side
    /// has seen the place of since.
    pub(crate) fn clear_remains(&mut self) {
        for b in 0..self.buildings.len() {
            let (State::Destroyed(fell), Some(gone)) =
                (self.buildings[b].state, self.remains_gone_at(b))
            else {
                continue;
            };
            // The first tick it's off the map: one that falls leaves no
            // sooner than the tick after.
            let leaving = self.ticks == gone.max(fell + 1);
            if !leaving && self.buildings[b].remembered.is_empty() {
                continue;
            }
            let unwatched =
                |p: u32| !self.sees_building(p, b) && self.building_seen(p, b) == Seen::Explored;
            let remembered: Vec<u32> = match leaving {
                true => (self.sides.players.iter().map(|p| p.id))
                    .filter(|&p| unwatched(p))
                    .collect(),
                false => (self.buildings[b].remembered.iter().copied())
                    .filter(|&p| unwatched(p))
                    .collect(),
            };
            self.buildings[b].remembered = remembered;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::*;
    use crate::{Left, SECOND, State};
    use tgmap::{ANY_DAMAGE, Death, Id};

    fn death(filter: i32, threshold: f32, length: f32) -> Death {
        Death {
            ids: Id::from(format!("die{length}")),
            filter,
            threshold,
            weight: 1.0,
            length,
        }
    }

    /// What the unit at 0 dies playing, as the last hit it took was.
    fn dies(deaths: Vec<Death>, struck: Option<(i32, f32)>) -> u64 {
        let mut w = battle(&[1, 2], vec![soldier(vec![]), soldier(vec![])], at_war());
        w.rules
            .animation_sets
            .insert("soldier".into(), plays(deaths));
        if let Some((filter, damage)) = struck {
            hit_by(&mut w, 0, 1);
            let s = w.units[0].struck.as_mut().unwrap();
            (s.filter, s.damage) = (filter, damage);
        }
        w.pick_death(0);
        w.units[0].dying
    }

    #[test]
    fn a_death_is_picked_by_the_last_hit() {
        // One alone plays whatever the damage, if its kind fits.
        assert_eq!(dies(vec![death(0, 25.0, 1.0)], Some((0, 1.0))), SECOND);
        assert_eq!(dies(vec![death(0, 0.0, 1.0)], Some((2, 50.0))), 0);
        // Of several, the one for the most damage the hit reaches.
        let several = || vec![death(2, 0.0, 1.0), death(2, 25.0, 2.0), death(0, 0.0, 3.0)];
        assert_eq!(dies(several(), Some((2, 30.0))), 2 * SECOND);
        assert_eq!(dies(several(), Some((2, 10.0))), SECOND);
        assert_eq!(dies(several(), Some((1, 10.0))), 0);
        // Never hit, it bleeds to death, which only one for any fits.
        assert_eq!(dies(several(), None), 0);
        assert_eq!(dies(vec![death(ANY_DAMAGE, 0.0, 4.0)], None), 4 * SECOND);
        // Those as good as each other are picked between.
        let even = vec![death(2, 0.0, 1.0), death(2, 0.0, 2.0)];
        assert!([SECOND, 2 * SECOND].contains(&dies(even, Some((2, 5.0)))));
    }

    #[test]
    fn remains_start_lying_once_the_death_has_played() {
        let mut w = battle(&[1, 2], vec![soldier(vec![]), soldier(vec![])], at_war());
        w.rules.remains.insert("soldier".into(), [1.0, 1.0]);
        w.units[0].died = Some(0);
        w.units[0].death = Some(Id::from("die1"));
        w.units[0].dying = SECOND;
        w.ticks = 3 * SECOND - 1;
        assert!(w.unit_on_map(0));
        w.ticks = 3 * SECOND;
        assert!(!w.unit_on_map(0));
    }

    #[test]
    fn the_dead_lie_until_their_remains_are_taken_off() {
        let mut w = battle(&[1, 2], vec![soldier(vec![]), soldier(vec![])], at_war());
        w.rules.remains.insert("soldier".into(), [1.0, 1.0]);
        w.units[0].died = Some(0);
        assert!(w.unit_on_map(0) && !w.organization_on_map(0));
        w.ticks = 2 * SECOND - 1;
        assert!(w.unit_on_map(0));
        w.ticks = 2 * SECOND;
        assert!(!w.unit_on_map(0));
        // What leaves without dying goes at once.
        w.units[1].left = Some(Left::Disbanded(w.ticks));
        assert!(!w.unit_on_map(1));
        w.units[1].left = None;
        w.units[1].awaited = true;
        assert!(!w.unit_on_map(1));
    }

    #[test]
    fn a_side_remembers_remains_it_didnt_see_go_until_it_looks_again() {
        let house = building(9, 2, [40.0, 10.0]);
        let mut w = with_buildings(battle(&[1, 2], vec![soldier(vec![])], at_war()), &[house]);
        w.rules.sights.insert(
            "squad".into(),
            tgmap::Sight {
                range: 8.0,
                remote: 0.0,
            },
        );
        w.set_fog(tgmap::FogSetting::Full);
        w.rules.remains.insert("house".into(), [0.5, 0.5]);
        let put = |w: &mut crate::World, at: [f32; 2]| {
            w.units[0].position = at;
            w.units[0].previous = at;
        };
        put(&mut w, [38.0, 10.0]);
        w.tick();
        put(&mut w, [10.0, 70.0]);
        w.tick();
        w.buildings[0].state = State::Destroyed(w.ticks);
        let mut watched = w.clone();
        // Even remains that go the moment it falls are remembered.
        let mut instant = w.clone();
        instant.rules.remains.insert("house".into(), [0.0, 0.0]);
        instant.tick();
        assert_eq!(instant.buildings[0].remembered, [1, 3]);
        for _ in 0..SECOND {
            w.tick();
        }
        // Player 3 shares player 1's sight.
        assert_eq!(w.buildings[0].remembered, [1, 3]);
        assert!(w.building_on_map(0));
        put(&mut w, [38.0, 10.0]);
        w.tick();
        assert!(w.buildings[0].remembered.is_empty());
        assert!(!w.building_on_map(0));
        // One seen as it goes is gone for good.
        put(&mut watched, [38.0, 10.0]);
        for _ in 0..SECOND {
            watched.tick();
        }
        assert!(!watched.building_on_map(0));
    }
}
