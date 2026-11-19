//! The stances units take: the last of their template's stances whose
//! health and states they meet, which gives them its property while it's
//! held.

use tgmap::{AnimationSet, Id};

use crate::{TICK, World};

/// The states a unit can be in, as stances and effects name them.
const ENGAGED: &str = "mind_engaged";
const UNDER_SIEGE: &str = "under_siege";
const FALLING: &str = "falling";
const DEAD: &str = "dead";

impl World {
    /// The states the unit at `u` is in: dead, falling under its
    /// parachute, attacking a target of its own other than by bombardment,
    /// and under siege.
    pub fn unit_states(&self, u: usize) -> Vec<&'static str> {
        [DEAD, FALLING, ENGAGED, UNDER_SIEGE]
            .into_iter()
            .filter(|s| self.in_state(u, s))
            .collect()
    }

    /// Whether the unit at `u` is in `state`, as [`World::unit_states`]
    /// has them; a dead one is in no other.
    fn in_state(&self, u: usize, state: &str) -> bool {
        let unit = &self.units[u];
        // Not one that left the map by unpacking, which is no death.
        let dead = unit.died.is_some();
        let is = |s: &str| state.eq_ignore_ascii_case(s);
        if is(DEAD) {
            return dead;
        }
        if dead {
            return false;
        }
        let bombarding = self.organizations[unit.organization].bombarding;
        (is(FALLING) && unit.descent.is_some())
            || (is(ENGAGED) && unit.alive() && unit.target.is_some() && !bombarding)
            || (is(UNDER_SIEGE) && unit.besieged.is_some())
    }

    /// The health share and states the unit at `u`'s stance is decided by.
    fn stance_heard(&self, u: usize) -> (u32, u8) {
        let full = self.max_health(u);
        let share = match full > 0.0 {
            true => self.units[u].health / full,
            false => f32::NAN,
        };
        let states = [DEAD, FALLING, ENGAGED, UNDER_SIEGE]
            .into_iter()
            .enumerate();
        let mask = states.fold(0, |m, (i, s)| m | (u8::from(self.in_state(u, s)) << i));
        (share.to_bits(), mask)
    }

    /// The stance that applies to the unit at `u` as it is now: the last
    /// of its template's whose health and states it meets.
    fn applying_stance(&self, u: usize) -> Option<Id> {
        let unit = &self.units[u];
        let stances = self.rules.stances.get(&unit.template)?;
        let full = self.max_health(u);
        let share = match full > 0.0 {
            true => unit.health / full,
            false => return None,
        };
        let applies = |s: &&tgmap::Stance| {
            share <= s.health && s.states.iter().all(|n| self.in_state(u, n.as_str()))
        };
        stances.iter().rfind(applies).map(|s| s.ids.clone())
    }

    /// Decides each living unit's stance afresh whenever its health or
    /// states change, and has one whose change was waiting take it once
    /// the animation it was playing ends. A waiting change isn't taken
    /// back by another decision in the meantime: that one is lost.
    pub(crate) fn keep_stances(&mut self) {
        for u in 0..self.units.len() {
            let unit = &self.units[u];
            // Its death settles its last.
            if unit.died.is_some() || !self.rules.stances.contains_key(&unit.template) {
                continue;
            }
            let ticks = self.ticks;
            let heard = self.stance_heard(u);
            let unit = &mut self.units[u];
            if ticks >= unit.playing
                && let Some(due) = unit.stance_due.take()
            {
                unit.stance = due;
            }
            if unit.stance_heard.replace(heard) == Some(heard) || unit.stance_due.is_some() {
                continue;
            }
            let stance = self.applying_stance(u);
            self.change_stance(u, stance);
        }
    }

    /// Has the unit at `u` take `stance`, at once if it can, else once
    /// the animation it's playing ends.
    fn change_stance(&mut self, u: usize, stance: Option<Id>) {
        let unit = &self.units[u];
        if unit.stance == stance {
            return;
        }
        let now = self.changes_at_once(u, stance.as_ref());
        let unit = &mut self.units[u];
        match now {
            true => unit.stance = stance,
            false => unit.stance_due = Some(stance),
        }
    }

    /// Whether the unit at `u` can change from its stance to `stance` at
    /// once: it has no animations, both are played from the same set,
    /// or what it's playing loops.
    fn changes_at_once(&self, u: usize, stance: Option<&Id>) -> bool {
        let unit = &self.units[u];
        let held = self.animation_set(u, unit.stance.as_ref());
        held.is_none() || self.ticks >= unit.playing || held == self.animation_set(u, stance)
    }

    /// The index among the unit at `u`'s animation sets of the one it
    /// plays while it holds `stance`: its own for its plain look, else
    /// the look's, else its last. `None` for one without animations.
    fn animation_set(&self, u: usize, stance: Option<&Id>) -> Option<usize> {
        let template = &self.units[u].template;
        let sets = self.rules.animation_sets.get(template)?;
        let stances = self.rules.stances.get(template);
        let look = stance.and_then(|ids| stances?.iter().find(|s| &s.ids == ids)?.look.as_ref());
        Some(match look {
            None => 0,
            Some(look) => {
                (sets.iter().position(|s| s.look.as_ref() == Some(look))).unwrap_or(sets.len() - 1)
            }
        })
    }

    /// The animations the unit at `u` plays while it holds `stance`.
    pub(crate) fn animations_in(&self, u: usize, stance: Option<&Id>) -> Option<&AnimationSet> {
        let sets = self.rules.animation_sets.get(&self.units[u].template)?;
        sets.get(self.animation_set(u, stance)?)
    }

    /// Has the unit at `u` start playing the animation `ids` once, as it
    /// uses an ability, in the set of the stance it holds.
    pub(crate) fn play_once(&mut self, u: usize, ids: Option<&str>) {
        let unit = &self.units[u];
        let set = self.animations_in(u, unit.stance.as_ref());
        let length = ids.and_then(|ids| set?.lengths.get(&Id::from(ids)));
        if let Some(&length) = length {
            self.units[u].playing = self.ticks + (length.max(0.0) / TICK).round() as u64;
        }
    }

    /// The stance the unit at `u` dies into, and whether it changes to
    /// it at once; any change it was waiting on is forgotten.
    pub(crate) fn dying_stance(&mut self, u: usize) -> (Option<Id>, bool) {
        self.units[u].stance_due = None;
        let into = self.applying_stance(u);
        let now = self.changes_at_once(u, into.as_ref());
        (into, now)
    }

    /// The look the unit at `u` is drawn with, by its `[Aspect]`'s ids:
    /// its stance's; `None` for its plain look.
    pub fn stance_look(&self, u: usize) -> Option<&Id> {
        let unit = &self.units[u];
        let ids = unit.stance.as_ref()?;
        let stances = self.rules.stances.get(&unit.template)?;
        stances.iter().find(|s| &s.ids == ids)?.look.as_ref()
    }

    /// The property the unit at `u`'s stance gives it, if it holds one that
    /// gives one it has what it needs for.
    pub(crate) fn stance_property(&self, u: usize) -> Option<&Id> {
        let unit = &self.units[u];
        let ids = unit.stance.as_ref()?;
        let stances = self.rules.stances.get(&unit.template)?;
        let property = stances.iter().find(|s| &s.ids == ids)?.property.as_ref()?;
        let needs = self.rules.combat_effects.get(property).map(|e| &e.requires);
        let kind = self.rules.kinds.get(&unit.template);
        let has = |p: &Id| kind.is_some_and(|k| k.properties.contains(p));
        needs.is_none_or(|n| n.iter().all(has)).then_some(property)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::*;
    use crate::{SECOND, TICK, Target};

    use tgmap::{AnimationSet, CombatEffect, Death, Id, Stance};

    /// A soldier of player 1's that kneels while fighting back under fire,
    /// and goes prone at half health or less, slowed as AA's are. Its
    /// shot takes a second standing; kneeling, half that. It dies in
    /// one of two ways standing, by fire or anything, and in one way
    /// kneeling, by anything but fire.
    fn kneeling() -> crate::World {
        let mut w = battle(
            &[1, 2],
            vec![soldier(Vec::new()), soldier(Vec::new())],
            at_war(),
        );
        let stance = |ids: &str, health, property: &str| Stance {
            ids: ids.into(),
            health,
            states: vec!["mind_engaged".into(), "under_siege".into()],
            property: Some(property.into()),
            look: Some(ids.into()),
        };
        let template = w.units[0].template.clone();
        (w.rules.stances).insert(
            template.clone(),
            vec![
                stance("kneel", 1.0, "infantry_kneel"),
                stance("prone", 0.5, "infantry_prone"),
            ],
        );
        for (p, speed) in [("infantry_kneel", -0.2), ("infantry_prone", -0.6)] {
            let effect = CombatEffect {
                speed,
                ..CombatEffect::default()
            };
            w.rules.combat_effects.insert(Id::from(p), effect);
        }
        w.rules.damage_filters.insert("fire".into(), 3);
        let death = |ids: &str, filter, length| Death {
            ids: ids.into(),
            filter,
            threshold: 0.0,
            weight: 1.0,
            length,
        };
        let set = |look: Option<&str>, attack: f32, deaths| AnimationSet {
            look: look.map(Id::from),
            deaths,
            lengths: [(Id::from("attack1"), attack)].into(),
        };
        let sets = vec![
            set(
                None,
                1.0,
                vec![
                    death("burn", 3, 3.0),
                    death("die1", 0, 1.0),
                    death("die9", 0, 1.0),
                ],
            ),
            set(Some("kneel"), 0.5, vec![death("die9", 0, 2.0)]),
        ];
        w.rules.animation_sets.insert(template, sets);
        w
    }

    /// Has the soldier at 0 fire on the one at 1 and be fired on.
    fn under_fire(w: &mut crate::World) {
        w.units[0].target = Some(Target::Unit(1));
        w.units[0].besieged = Some(w.ticks + (7.0 / TICK) as u64);
    }

    fn shoot(w: &mut crate::World) {
        w.play_once(0, Some("attack1"));
    }

    fn speed(w: &crate::World) -> f32 {
        w.boost(0).speed
    }

    #[test]
    fn infantry_kneel_only_while_fighting_back_under_fire_and_go_prone_when_hurt() {
        let mut w = kneeling();
        let held = |w: &mut crate::World| {
            w.keep_stances();
            w.stance_property(0).cloned()
        };
        assert_eq!(held(&mut w), None);
        // Firing without being hit, or hit without firing, it stands.
        w.units[0].target = Some(Target::Unit(1));
        assert_eq!(held(&mut w), None);
        w.units[0].target = None;
        w.units[0].besieged = Some(w.ticks + (7.0 / TICK) as u64);
        assert_eq!(held(&mut w), None);
        w.units[0].target = Some(Target::Unit(1));
        assert_eq!(held(&mut w), Some(Id::from("infantry_kneel")));
        assert_eq!(w.stance_look(0), Some(&Id::from("kneel")));
        assert!((speed(&w) + 0.2).abs() < 1e-6);
        w.units[0].health = 0.5 * w.max_health(0);
        assert_eq!(held(&mut w), Some(Id::from("infantry_prone")));
        assert!((speed(&w) + 0.6).abs() < 1e-6);
        // Bombarding is no fight of its own.
        w.organizations[0].bombarding = true;
        assert_eq!(held(&mut w), None);
    }

    #[test]
    fn a_man_mid_shot_finishes_it_in_his_old_stance() {
        let mut w = kneeling();
        shoot(&mut w);
        under_fire(&mut w);
        w.keep_stances();
        w.ticks += SECOND - 1;
        w.keep_stances();
        assert_eq!(w.units[0].stance, None);
        assert_eq!(speed(&w), 0.0);
        w.ticks += 1;
        w.keep_stances();
        assert_eq!(w.units[0].stance, Some(Id::from("kneel")));
        // Kneeling, his shot is the kneeling one's.
        shoot(&mut w);
        assert_eq!(w.units[0].playing, w.ticks + SECOND / 2);
    }

    #[test]
    fn a_waiting_change_isnt_taken_back() {
        let mut w = kneeling();
        shoot(&mut w);
        under_fire(&mut w);
        w.keep_stances();
        // The fight's over before his shot is.
        w.units[0].target = None;
        w.units[0].besieged = None;
        w.keep_stances();
        w.ticks += SECOND;
        w.keep_stances();
        assert_eq!(w.units[0].stance, Some(Id::from("kneel")));
        // Till his stance is next decided.
        w.units[0].health *= 0.9;
        w.keep_stances();
        assert_eq!(w.units[0].stance, None);
    }

    #[test]
    fn a_change_within_one_animation_set_waits_for_nothing() {
        let mut w = kneeling();
        for s in w.rules.stances.values_mut().flatten() {
            s.look = Some("kneel".into());
        }
        under_fire(&mut w);
        w.keep_stances();
        shoot(&mut w);
        w.units[0].health = 0.5 * w.max_health(0);
        w.keep_stances();
        assert_eq!(w.units[0].stance, Some(Id::from("prone")));
    }

    /// The soldier at 0 dies of `filter` damage, kneeling, mid-shot or
    /// not.
    fn dies_kneeling(mut w: crate::World, mid_shot: bool, filter: i32) -> crate::World {
        under_fire(&mut w);
        w.keep_stances();
        if mid_shot {
            shoot(&mut w);
        }
        hit_by(&mut w, 0, 1);
        w.units[0].struck.as_mut().unwrap().filter = filter;
        w.units[0].health = 0.0;
        w.units[0].die(w.ticks);
        w.pick_death(0);
        w
    }

    #[test]
    fn a_man_dying_between_shots_stands_and_dies_as_one_standing() {
        let w = dies_kneeling(kneeling(), false, 3);
        assert_eq!(w.units[0].stance, None);
        assert_eq!(w.units[0].death, Some(Id::from("burn")));
        assert_eq!(w.units[0].dying, 3 * SECOND);
    }

    #[test]
    fn a_man_dying_mid_shot_dies_as_one_kneeling_and_stands_as_it_starts() {
        let w = dies_kneeling(kneeling(), true, 0);
        assert_eq!(w.units[0].stance, None);
        // The standing one of that name plays, for the kneeling one's time.
        assert_eq!(w.units[0].death, Some(Id::from("die9")));
        assert_eq!(w.units[0].dying, 2 * SECOND);
        // With no namesake, the standing set's first.
        let mut w = kneeling();
        let sets = w.rules.animation_sets.values_mut().next().unwrap();
        sets[1].deaths[0].ids = "die4".into();
        let w = dies_kneeling(w, true, 0);
        assert_eq!(w.units[0].death, Some(Id::from("burn")));
    }

    #[test]
    fn a_man_with_no_kneeling_death_to_fit_stays_kneeling() {
        let w = dies_kneeling(kneeling(), true, 3);
        assert_eq!(w.units[0].stance, Some(Id::from("kneel")));
        assert_eq!((w.units[0].death.clone(), w.units[0].dying), (None, 0));
    }
}
