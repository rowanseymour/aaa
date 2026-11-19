//! The experience a side's faction earns, and spending it on the faction's
//! special operations; and the experience its organizations earn, and the
//! levels it raises them to.

use tgmap::{CombatEffect, Id, Operation};

use crate::supply::BEAT;
use crate::value::MULTIPLY_FLOOR;
use crate::{LANDINGS_KEPT, Notice, Piece, Refused, Relation, SECOND, Struck, TICK, Target, World};

/// Gains smaller than this are dropped.
const LEAST_GAIN: f32 = 0.0001;

/// A side's faction: their experience and special operations.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Commander {
    pub experience: f32,
    /// The most it can hold, if there's a ceiling.
    pub max: Option<f32>,
    /// The faction's special operations, in the order they list them.
    pub operations: Vec<SpecialOp>,
}

/// A special operation carried out, for what shows it to find.
#[derive(Debug, Clone, PartialEq)]
pub struct Operated {
    pub tick: u64,
    pub player: u32,
    pub operation: Id,
    /// Its centre, for one aimed somewhere on the map.
    pub at: Option<[f32; 2]>,
}

/// One of a faction's special operations, and how far it's recharged.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecialOp {
    pub id: Id,
    /// The experience using it takes.
    pub cost: f32,
    /// Seconds after use before it's ready again.
    pub recharge: f32,
    /// Seconds before it's ready again, 0 once it is.
    pub left: f32,
}

impl SpecialOp {
    /// One of `id`, ready, as every operation starts a match.
    pub fn new(id: Id, cost: f32, recharge: f32) -> Self {
        SpecialOp {
            id,
            cost,
            recharge,
            left: 0.0,
        }
    }

    /// How far it's recharged, from 0 straight after use to 1 once ready.
    pub fn charged(&self) -> f32 {
        match self.recharge > 0.0 {
            true => 1.0 - self.left / self.recharge,
            false => 1.0,
        }
    }
}

impl Commander {
    fn set(&mut self, experience: f32) {
        let max = self.max.unwrap_or(f32::INFINITY);
        self.experience = experience.clamp(0.0, max);
    }
}

impl World {
    /// Gives `player` a faction whose side holds at most `max`, starting
    /// with `experience`, with `operations` to spend it on.
    pub fn lead(
        &mut self,
        player: u32,
        experience: f32,
        max: Option<f32>,
        operations: Vec<SpecialOp>,
    ) {
        let mut commander = Commander {
            experience: 0.0,
            max,
            operations,
        };
        commander.set(experience);
        self.commanders.insert(player, commander);
    }

    /// `player`'s experience, for a side with a faction.
    pub fn experience(&self, player: u32) -> Option<f32> {
        self.commanders.get(&player).map(|c| c.experience)
    }

    /// `player`'s faction's `i`th special operation.
    pub fn special_op(&self, player: u32, i: usize) -> Option<&SpecialOp> {
        self.commanders.get(&player)?.operations.get(i)
    }

    /// Whether `player` may use their faction's `i`th special operation
    /// now: it's one the match can carry out, it has recharged, and the
    /// side has the experience for it.
    pub(crate) fn may_operate(&self, player: u32, i: usize) -> Result<(), Refused> {
        let commander = self.commanders.get(&player).ok_or(Refused::NotYours)?;
        let op = commander.operations.get(i).ok_or(Refused::NotYours)?;
        if !self.rules.operations.contains_key(&op.id) {
            return Err(Refused::Unable);
        }
        if op.left > 0.0 {
            return Err(Refused::Recharging);
        }
        if commander.experience < op.cost {
            return Err(Refused::Unaffordable);
        }
        match &self.rules.operations[&op.id] {
            Operation::Summon(summon) => self.may_summon(player, &op.id, summon),
            _ => Ok(()),
        }
    }

    /// Uses `player`'s faction's `i`th special operation, aimed at `at` or
    /// given on `on`, spending its cost and starting it recharging only
    /// once it's been carried out.
    pub(crate) fn special_operation(
        &mut self,
        player: u32,
        i: usize,
        at: [f32; 2],
        on: Option<Piece>,
    ) -> Result<(), Refused> {
        self.may_operate(player, i)?;
        let id = self.commanders[&player].operations[i].id.clone();
        self.operate(player, &id, at, on)?;
        let commander = self.commanders.get_mut(&player).expect("checked");
        let op = &mut commander.operations[i];
        op.left = op.recharge;
        let cost = op.cost;
        commander.set(commander.experience - cost);
        // One used on the side itself is aimed nowhere.
        let aimed = !matches!(self.rules.operations[&id], Operation::Reveal { .. });
        self.operated.push(Operated {
            tick: self.ticks,
            player,
            operation: id,
            at: aimed.then_some(at),
        });
        Ok(())
    }

    /// Whether `player`'s minimap has its fog lifted, as Radar Scan does.
    pub fn revealed(&self, player: u32) -> bool {
        self.reveals.contains_key(&player)
    }

    /// Credits every side with a faction a tick's steady gain, and
    /// recharges their operations a tick's worth.
    pub(crate) fn lead_sides(&mut self) {
        let gain = self.rules.experience_per_second * TICK;
        for commander in self.commanders.values_mut() {
            commander.set(commander.experience + gain);
            for op in &mut commander.operations {
                op.left = (op.left - TICK).max(0.0);
            }
        }
        let ticks = self.ticks;
        self.operated.retain(|o| o.tick + LANDINGS_KEPT > ticks);
        // A side's property goes at its once-a-second look after its time.
        if ticks.is_multiple_of(SECOND) {
            self.reveals.retain(|_, until| *until > ticks);
        }
    }
}

/// The kinds of event that earn an organization experience, each by its
/// own multiplier.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Feat {
    Kill,
    Capture,
    Rout,
    /// A second's engagement, which isn't kept within the gains' bounds.
    Engagement,
}

impl World {
    /// The levels the organization at `r` can earn, if it earns
    /// experience at all.
    fn levels(&self, r: usize) -> Option<&[tgmap::Level]> {
        let template = &self.organizations[r].template;
        let levels = self.rules.experience_levels.get(template)?;
        Some(&levels.levels)
    }

    /// The property the organization at `r`'s level gives it, and every
    /// member of it.
    pub fn level_property(&self, r: usize) -> Option<&Id> {
        let level = self.organizations[r].level;
        Some(&self.levels(r)?.get(level)?.property)
    }

    /// The experience the organization at `r`'s next level takes, if it has
    /// one to go.
    pub fn next_level(&self, r: usize) -> Option<f32> {
        let next = self.organizations[r].level + 1;
        Some(self.levels(r)?.get(next)?.required)
    }

    /// What the properties something of `owner`'s made from `template`
    /// and holding `held` adds to its fighting, with those of `more` as
    /// well: its template's own, its side's technologies on its kind and
    /// those it holds for a time.
    pub(crate) fn held_effect<'a>(
        &'a self,
        owner: u32,
        template: &Id,
        held: impl IntoIterator<Item = &'a crate::Held> + 'a,
        more: &[Option<&Id>],
    ) -> CombatEffect {
        let table = &self.rules.combat_effects;
        let kind = self.rules.kinds.get(template);
        let own = kind.into_iter().flat_map(|k| &k.properties);
        let bearing = self.bearing(table, |e| &e.requires, owner, template, held);
        let more = more.iter().flatten().filter_map(|&p| table.get(p));
        (own.filter_map(|p| table.get(p)).chain(bearing).chain(more))
            .fold(CombatEffect::default(), CombatEffect::with)
    }

    /// What the properties the unit at `u` holds add to its fighting: its
    /// template's own, its side's technologies on its kind, those it and
    /// its organization hold for a time, its organization's level and its
    /// stance's.
    pub(crate) fn boost(&self, u: usize) -> CombatEffect {
        let unit = &self.units[u];
        let r = unit.organization;
        let owner = self.organizations[r].owner;
        let held = self.timed_on(crate::Target::Unit(u));
        let more = [self.level_property(r), self.stance_property(u)];
        self.held_effect(owner, &unit.template, held, &more)
    }

    /// The most health the unit at `u` can have.
    pub fn max_health(&self, u: usize) -> f32 {
        let unit = &self.units[u];
        let base = unit.fighter.as_ref().map_or(0.0, |f| f.health);
        base * unit.health_factor
    }

    /// Rescales the unit at `u`'s maximum health to what its properties
    /// make it now, keeping its share of health, never below 1 by this.
    fn rescale_health(&mut self, u: usize) {
        let factor = (1.0 + self.boost(u).health).max(MULTIPLY_FLOOR);
        let unit = &mut self.units[u];
        if factor == unit.health_factor {
            return;
        }
        if unit.died.is_none() && unit.fighter.is_some() {
            unit.health = (unit.health * factor / unit.health_factor).max(1.0);
        }
        unit.health_factor = factor;
    }

    /// The most health the building at `b` can have.
    pub fn building_max_health(&self, b: usize) -> f32 {
        let building = &self.buildings[b];
        building.fighter.health * building.health_factor
    }

    /// Rescales the building at `b`'s maximum health as
    /// [`World::rescale_health`] does a unit's.
    fn rescale_building_health(&mut self, b: usize) {
        let factor = (1.0 + self.building_boost(b).health).max(MULTIPLY_FLOOR);
        let building = &mut self.buildings[b];
        if factor == building.health_factor {
            return;
        }
        if building.health > 0.0 {
            building.health = (building.health * factor / building.health_factor).max(1.0);
        }
        building.health_factor = factor;
    }

    /// Rescales every unit's and building's maximum health to what its
    /// properties make it now, as technologies arrive and properties come
    /// and go.
    pub(crate) fn keep_health(&mut self) {
        for u in 0..self.units.len() {
            self.rescale_health(u);
        }
        for b in 0..self.buildings.len() {
            self.rescale_building_health(b);
        }
    }

    /// The level `experience` reaches among `levels`.
    fn level_for(levels: &[tgmap::Level], experience: f32) -> usize {
        let reached = levels.iter().take_while(|l| l.required <= experience);
        reached.count().saturating_sub(1)
    }

    /// Sets the organization at `r`'s experience, as a map stores it, and
    /// its level to what that reaches, telling nobody.
    pub(crate) fn set_experience(&mut self, r: usize, experience: f32) {
        let Some(levels) = self.levels(r) else {
            return;
        };
        let level = Self::level_for(levels, experience.max(0.0));
        self.organizations[r].experience = experience.max(0.0);
        self.raise_to(r, level);
    }

    /// Puts the organization at `r` at `level`, rescaling its members'
    /// health at once. Its maximum morale waits for the next time it's
    /// worked out.
    fn raise_to(&mut self, r: usize, level: usize) {
        self.organizations[r].level = level;
        for u in self.organizations[r].units.clone() {
            self.rescale_health(u);
        }
    }

    /// Gives the organization at `r` `amount` experience, if it earns any,
    /// and its side's faction its share; raising it a level or more where
    /// that reaches them.
    pub(crate) fn gain_experience(&mut self, r: usize, amount: f32) {
        if amount < LEAST_GAIN || self.levels(r).is_none() {
            return;
        }
        let owner = self.organizations[r].owner;
        let experience = self.organizations[r].experience + amount;
        self.organizations[r].experience = experience;
        let share = amount * self.rules.experience_gains.faction_gain_multiplier;
        if let Some(commander) = self.commanders.get_mut(&owner) {
            commander.set(commander.experience + share);
        }
        let levels = self.levels(r).expect("checked");
        let level = Self::level_for(levels, experience);
        if level > self.organizations[r].level {
            self.raise_to(r, level);
            self.notices.push(Notice::Levelled {
                player: owner,
                organization: r,
                level,
            });
        }
    }

    /// What `feat` earns a gainer worth `gainer` against a victim worth
    /// `victim`, made from `template`.
    fn worth(&self, feat: Feat, victim: f32, gainer: f32, template: &Id) -> f32 {
        let gains = &self.rules.experience_gains;
        if gainer <= 0.0 {
            return 0.0;
        }
        let faked =
            (self.rules.experience_levels.get(template)).is_some_and(|l| l.fake_value.is_some());
        let victim = match () {
            _ if victim > 0.0 => victim,
            _ if faked => gains.default_value,
            _ => -1.0,
        };
        let base = 0.5 + 0.5 * victim / gainer;
        let (multiplier, kept) = match feat {
            Feat::Kill => (gains.kill_multiplier, true),
            Feat::Capture => (gains.capture_multiplier, true),
            Feat::Rout => (gains.rout_multiplier, true),
            Feat::Engagement => (gains.engage_multiplier, false),
        };
        let amount = base * multiplier;
        match kept {
            true => amount.clamp(gains.gain_minimum, gains.gain_maximum),
            false => amount,
        }
    }

    /// What's credited with killing something of `owner`'s whose last
    /// enemy hit was `struck`: what fired it, if it came recently enough
    /// and its side is still an enemy. A special operation's credits no
    /// one.
    pub(crate) fn killer(&self, struck: Option<Struck>, owner: u32) -> Option<Target> {
        let struck = struck?;
        let window = (self.rules.kill_credit_seconds / TICK).round() as u64;
        let recent = self.ticks - struck.at <= window;
        let enemy = self.sides.relation(struck.by, owner) == Relation::Enemy;
        struck.from.filter(|_| recent && enemy)
    }

    /// What the organization of the unit at `k` earns for killing the unit
    /// at `u`, the two worth `[victim, killer]`.
    pub(crate) fn credit_kill(&mut self, u: usize, k: usize, [victim, killer]: [f32; 2]) {
        let template = self.units[u].template.clone();
        let gain = self.worth(Feat::Kill, victim, killer, &template);
        self.gain_experience(self.units[k].organization, gain);
    }

    /// What the organization of whatever last hit one of the organization
    /// at `r`'s members earns for destroying it, if a unit did.
    pub(crate) fn credit_destruction(&mut self, r: usize) {
        let organization = &self.organizations[r];
        let by = self.killer(organization.struck, organization.owner);
        let Some(Target::Unit(k)) = by else {
            return;
        };
        let killers = self.units[k].organization;
        let template = organization.template.clone();
        let (victim, gainer) = (self.full_value(r), self.full_value(killers));
        let gain = self.worth(Feat::Kill, victim, gainer, &template);
        self.gain_experience(killers, gain);
    }

    /// What the organization of `by`, a unit, earns for bringing down the
    /// building at `b`: taking it compares the building with the whole
    /// organization, destroying it with the unit alone.
    pub(crate) fn credit_fall(&mut self, b: usize, by: Target, captured: bool) {
        let Target::Unit(k) = by else {
            return;
        };
        let (template, r) = (
            self.buildings[b].template.clone(),
            self.units[k].organization,
        );
        let victim = self.target_value(Target::Building(b));
        let (feat, gainer) = match captured {
            true => (Feat::Capture, self.full_value(r)),
            false => (Feat::Kill, self.unit_value(k)),
        };
        let gain = self.worth(feat, victim, gainer, &template);
        self.gain_experience(r, gain);
    }

    /// What each organization engaging the one at `r`, which is routing,
    /// earns for breaking it.
    pub(crate) fn credit_rout(&mut self, r: usize) {
        let template = self.organizations[r].template.clone();
        let victim = self.full_value(r);
        let gains: Vec<(usize, f32)> = (self.engagers(r))
            .map(|e| {
                (
                    e,
                    self.worth(Feat::Rout, victim, self.full_value(e), &template),
                )
            })
            .collect();
        for (e, gain) in gains {
            self.gain_experience(e, gain);
        }
    }

    /// Plays a tick of organizations' experience: on each one's beat, what
    /// a second of staying engaged earns it.
    pub(crate) fn keep_experience(&mut self) {
        for r in 0..self.organizations.len() {
            if !(self.ticks + r as u64).is_multiple_of(BEAT) {
                continue;
            }
            let organization = &self.organizations[r];
            let target = organization.engaged.first().copied();
            let engaging = target.is_some() && !self.destroyed(r);
            let stayed = engaging && organization.engaging_before;
            self.organizations[r].engaging_before = engaging;
            let Some(target) = target.filter(|_| stayed) else {
                continue;
            };
            let template = match target {
                Piece::Organization(t) => self.organizations[t].template.clone(),
                Piece::Building(b) => self.buildings[b].template.clone(),
            };
            let victim = self.piece_value(target);
            let gain = self.worth(Feat::Engagement, victim, self.full_value(r), &template);
            self.gain_experience(r, gain);
        }
    }
}

crate::hash::digest!(Operated {
    tick,
    player,
    operation,
    at
});
crate::hash::digest!(Commander {
    experience,
    max,
    operations
});
crate::hash::digest!(SpecialOp { left } skip { id, cost, recharge });

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Command;

    fn world() -> World {
        let pathing = tgmap::Pathing::uniform(8, tgmap::Domain::Land);
        World::from_organizations(crate::Sides::default(), pathing, &[])
    }

    #[test]
    fn sides_with_a_faction_gain_experience_up_to_its_ceiling() {
        let mut w = world();
        w.rules.experience_per_second = 1.0;
        w.lead(1, 200.0, Some(5000.0), Vec::new());
        w.lead(2, 4999.5, Some(5000.0), Vec::new());
        w.lead(3, 9000.0, None, Vec::new());
        for _ in 0..10 {
            w.tick();
        }
        assert!((w.experience(1).unwrap() - 201.0).abs() < 1e-3);
        assert_eq!(w.experience(2), Some(5000.0));
        assert!((w.experience(3).unwrap() - 9001.0).abs() < 1e-2);
        // A side without a faction has none.
        assert_eq!(w.experience(4), None);
    }

    #[test]
    fn a_start_past_the_ceiling_is_kept_to_it() {
        let mut w = world();
        w.lead(1, 6000.0, Some(5000.0), Vec::new());
        w.lead(2, -5.0, Some(5000.0), Vec::new());
        assert_eq!(
            (w.experience(1), w.experience(2)),
            (Some(5000.0), Some(0.0))
        );
    }

    /// A side of `experience` with a resupply that costs 200 and takes 10
    /// seconds to recharge, and an operation the match can't carry out.
    fn operating(experience: f32) -> World {
        let mut w = world();
        let resupply = tgmap::Operation::Enchant(tgmap::Enchantment {
            targets: tgmap::Targets::default(),
            area: 15.0,
            max_targets: 4,
            property: Id::from("property_resupply"),
            seconds: 30.0,
            kind: None,
            morale: 0.0,
            damage: Vec::new(),
            delay: 0.0,
        });
        w.rules.operations.insert(Id::from("resupply"), resupply);
        let ops = vec![
            SpecialOp::new(Id::from("resupply"), 200.0, 10.0),
            SpecialOp::new(Id::from("code_breaker"), 0.0, 10.0),
        ];
        w.lead(1, experience, Some(5000.0), ops);
        w
    }

    fn operate(i: usize) -> Command {
        Command::Operate {
            operation: i,
            at: [4.0, 4.0],
            on: None,
        }
    }

    #[test]
    fn operations_take_their_cost_once_carried_out() {
        let mut w = operating(200.0);
        // Equal is enough, and every operation starts ready.
        assert_eq!(w.special_op(1, 0).unwrap().charged(), 1.0);
        w.apply(1, operate(0)).unwrap();
        assert_eq!(w.experience(1), Some(0.0));
        assert_eq!(w.special_op(1, 0).unwrap().charged(), 0.0);
        // One the match can't carry out spends nothing.
        assert_eq!(w.apply(1, operate(1)), Err(Refused::Unable));
        // Nor one the side hasn't, nor one for a side with no faction.
        assert_eq!(w.check(1, &operate(2)), Err(Refused::NotYours));
        assert_eq!(w.check(2, &operate(0)), Err(Refused::NotYours));
    }

    #[test]
    fn operations_recharge_in_game_seconds_and_need_the_experience() {
        let mut w = operating(450.0);
        w.apply(1, operate(0)).unwrap();
        // Recharging comes first, though the side is now short too.
        assert_eq!(w.check(1, &operate(0)), Err(Refused::Recharging));
        for _ in 0..(5.0 / TICK).round() as usize {
            w.tick();
        }
        assert!((w.special_op(1, 0).unwrap().charged() - 0.5).abs() < 1e-3);
        for _ in 0..(5.0 / TICK).round() as usize {
            w.tick();
        }
        assert_eq!(w.special_op(1, 0).unwrap().charged(), 1.0);
        w.apply(1, operate(0)).unwrap();
        assert_eq!(w.experience(1), Some(50.0));
        for _ in 0..(10.0 / TICK).round() as usize {
            w.tick();
        }
        assert_eq!(w.check(1, &operate(0)), Err(Refused::Unaffordable));
        assert_eq!(w.experience(1), Some(50.0));
    }

    #[test]
    fn operations_carried_out_are_kept_a_while_for_what_shows_them() {
        let mut w = operating(200.0);
        w.apply(1, operate(0)).unwrap();
        let carried_out = Operated {
            tick: 0,
            player: 1,
            operation: Id::from("resupply"),
            at: Some([4.0, 4.0]),
        };
        assert_eq!(w.operated, [carried_out]);
        // One refused isn't.
        assert!(w.apply(1, operate(0)).is_err());
        assert_eq!(w.operated.len(), 1);
        for _ in 0..LANDINGS_KEPT {
            w.tick();
        }
        assert_eq!(w.operated.len(), 1);
        w.tick();
        assert!(w.operated.is_empty());
    }

    #[test]
    fn radar_scan_lifts_the_minimap_fog_until_the_look_after_its_time() {
        let mut w = world();
        let reveal = tgmap::Operation::Reveal { seconds: 10.0 };
        w.rules.operations.insert(Id::from("radar_scan"), reveal);
        let ops = vec![SpecialOp::new(Id::from("radar_scan"), 0.0, 300.0)];
        w.lead(1, 0.0, None, ops);
        // A tick into a second, so its time is up between looks.
        w.tick();
        w.apply(1, operate(0)).unwrap();
        assert!(w.revealed(1) && !w.revealed(2));
        assert_eq!(w.operated[0].at, None);
        let ticks = |seconds: f32| (seconds / TICK).round() as usize;
        for _ in 0..ticks(10.0) {
            w.tick();
        }
        assert!(w.revealed(1));
        for _ in 0..ticks(1.0) {
            w.tick();
        }
        assert!(!w.revealed(1));
    }
}

#[cfg(test)]
pub(crate) mod regiment_tests {
    use super::*;
    use crate::testing::*;
    use tgmap::{Level, Levels, MoraleEffect};

    /// Player 1's and player 2's riflemen 10 apart, in each other's range
    /// and too weak to kill each other for a long while, each in a squad
    /// that's Regular at 10 experience and Veteran at 20.
    pub(crate) fn veterans() -> World {
        let mut w = battle(
            &[1, 2],
            vec![
                soldier(vec![rifle(12.0, 0.1)]),
                soldier(vec![rifle(12.0, 0.1)]),
            ],
            at_war(),
        );
        let level = |n: usize, required: f32| Level {
            required,
            property: Id::from(format!("squad_level_{n}")),
        };
        let levels = Levels {
            levels: vec![level(0, 0.0), level(1, 10.0), level(2, 20.0)],
            fake_value: None,
        };
        w.rules.experience_levels.insert("squad".into(), levels);
        let regular = CombatEffect {
            attack: 5.0,
            defense: 2.0,
            health: 0.5,
            ..Default::default()
        };
        let veteran = CombatEffect {
            attack: 10.0,
            defense: 4.0,
            health: 1.0,
            ..Default::default()
        };
        w.rules
            .combat_effects
            .insert("squad_level_1".into(), regular);
        w.rules
            .combat_effects
            .insert("squad_level_2".into(), veteran);
        let steady = MoraleEffect {
            morale: 3.0,
            recovery_rate: 0.5,
            ..Default::default()
        };
        w.rules
            .morale_effects
            .insert("squad_level_2".into(), steady);
        w.rules.morale_values.insert("soldier".into(), 10.0);
        for r in 0..w.organizations.len() {
            w.refigure_morale(r, true);
        }
        rate(&mut w, "soldier", 10.0);
        w
    }

    /// Has the unit at `k` deal the unit at `u` its death.
    fn kill(w: &mut World, u: usize, k: usize) {
        hit_by(w, u, k);
        w.mourn(u);
    }

    fn levelled(w: &World) -> Vec<(usize, usize)> {
        (w.notices().iter())
            .filter_map(|n| match *n {
                Notice::Levelled {
                    organization,
                    level,
                    ..
                } => Some((organization, level)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn organizations_go_up_levels_as_they_earn_and_their_side_earns_too() {
        let mut w = veterans();
        w.lead(1, 0.0, Some(5000.0), Vec::new());
        assert_eq!(w.organizations[0].level, 0);
        assert_eq!(w.level_property(0), Some(&Id::from("squad_level_0")));
        w.gain_experience(0, 9.9);
        assert_eq!(w.organizations[0].level, 0);
        w.gain_experience(0, 0.1);
        assert_eq!(w.organizations[0].level, 1);
        // Two at once are told of together.
        w.gain_experience(0, 15.0);
        assert_eq!(w.organizations[0].level, 2);
        assert_eq!(levelled(&w), [(0, 1), (0, 2)]);
        // Past the last level experience goes on growing.
        w.gain_experience(0, 100.0);
        assert!((w.organizations[0].experience - 125.0).abs() < 1e-4);
        assert_eq!(w.organizations[0].level, 2);
        assert!((w.experience(1).unwrap() - 125.0).abs() < 1e-4);
        // Gains too small to count are dropped, for the side too.
        w.gain_experience(0, 0.00005);
        assert!((w.experience(1).unwrap() - 125.0).abs() < 1e-4);
    }

    #[test]
    fn organizations_without_levels_earn_nothing() {
        let mut w = veterans();
        w.lead(1, 0.0, None, Vec::new());
        w.rules.experience_levels.clear();
        w.gain_experience(0, 50.0);
        assert_eq!(w.organizations[0].experience, 0.0);
        assert_eq!(w.experience(1), Some(0.0));
        assert_eq!(w.level_property(0), None);
    }

    #[test]
    fn a_level_adds_attack_defense_and_health_keeping_its_share() {
        let mut w = veterans();
        w.units[0].health = 50.0;
        let before = w.unit_value(0);
        w.gain_experience(0, 10.0);
        // Half health of half as much again.
        assert_eq!(w.max_health(0), 150.0);
        assert_eq!(w.units[0].health, 75.0);
        assert_eq!(w.effect(Target::Unit(0)).attack, 5.0);
        assert_eq!(w.effect(Target::Unit(1)).defense, 0.0);
        assert_eq!(w.effect(Target::Unit(1)).attack, 0.0);
        assert_eq!(w.effect(Target::Unit(0)).defense, 2.0);
        assert_eq!(w.defense(Target::Unit(0)), Some((2.0, 0.0)));
        // It's worth more for it.
        assert!(w.unit_value(0) > before);
        // The next level's replaces it rather than adding to it.
        w.gain_experience(0, 10.0);
        assert_eq!(w.max_health(0), 200.0);
        assert_eq!(w.units[0].health, 100.0);
        assert_eq!(w.effect(Target::Unit(0)).defense, 4.0);
    }

    #[test]
    fn a_levels_attack_reaches_what_it_fires_on() {
        let mut w = veterans();
        w.gain_experience(0, 10.0);
        w.units[1].fighter.as_mut().unwrap().weapons.clear();
        run(&mut w, 1.0);
        // 0.1 + 5 a shot.
        assert!((w.units[1].health - (100.0 - 5.1)).abs() < 1e-3);
    }

    #[test]
    fn a_levels_morale_waits_for_the_maximum_to_be_worked_out_again() {
        let mut w = veterans();
        assert_eq!(w.organizations[0].morale_max, 10.0);
        w.organizations[0].morale = 5.0;
        w.gain_experience(0, 20.0);
        assert_eq!(w.organizations[0].morale_max, 10.0);
        assert_eq!(w.organizations[0].morale, 5.0);
        // Then it counts for its one man and once more for the squad.
        w.refigure_morale(0, false);
        assert_eq!(w.organizations[0].morale_max, 16.0);
        assert_eq!(w.organizations[0].morale, 8.0);
    }

    #[test]
    fn a_level_never_leaves_a_unit_below_1_health() {
        let mut w = veterans();
        w.rules
            .combat_effects
            .get_mut(&Id::from("squad_level_1"))
            .unwrap()
            .health = -0.9;
        w.units[0].health = 5.0;
        w.gain_experience(0, 10.0);
        assert_eq!(w.units[0].health, 1.0);
    }

    #[test]
    fn destroying_a_building_is_a_kill_of_its_own_value() {
        let mut pillbox = building(20, 2, [10.0, 20.0]);
        pillbox.fighter.destructible = true;
        let mut house = building(21, 2, [10.0, 20.0]);
        house.fighter.destructible = false;
        for (target, destroyed) in [(pillbox, true), (house, false)] {
            let mut w = siege(&[target]);
            let levels = w_levels();
            w.rules.experience_levels.insert("squad".into(), levels);
            w.rules.experience_gains.engage_multiplier = 0.0;
            rate(&mut w, "soldier", 10.0);
            run(&mut w, 30.0);
            let gone = matches!(w.buildings[0].state, crate::State::Destroyed(_));
            assert_eq!(gone, destroyed);
            // Unrated, it's worth the least; one only disabled, nothing.
            let gain = match destroyed {
                true => 0.5 - 0.5 / w.unit_value(0),
                false => 0.0,
            };
            assert!((w.organizations[0].experience - gain).abs() < 1e-4);
        }
    }

    #[test]
    fn a_building_that_falls_soon_after_a_hit_is_the_hitters_kill() {
        for (seconds, credited) in [(1.0, true), (3.0, false)] {
            let mut pillbox = building(20, 2, [10.0, 20.0]);
            pillbox.fighter.destructible = true;
            let mut w = siege(&[pillbox]);
            w.rules.experience_levels.insert("squad".into(), w_levels());
            rate(&mut w, "soldier", 10.0);
            let by = w.organizations[w.units[0].organization].owner;
            w.buildings[0].struck = Some(Struck {
                from: Some(Target::Unit(0)),
                by,
                at: w.ticks,
                filter: tgmap::ANY_DAMAGE,
                damage: 1.0,
            });
            // Worn away rather than shot down.
            w.ticks += (seconds / TICK) as u64;
            w.fall(0, None);
            assert!(matches!(w.buildings[0].state, crate::State::Destroyed(_)));
            let gain = 0.5 - 0.5 / w.unit_value(0);
            let expected = if credited { gain } else { 0.0 };
            assert!((w.organizations[0].experience - expected).abs() < 1e-4);
        }
    }

    fn w_levels() -> Levels {
        Levels {
            levels: vec![Level {
                required: 0.0,
                property: "squad_level_0".into(),
            }],
            fake_value: None,
        }
    }

    #[test]
    fn rescaling_leaves_health_alone_where_nothing_changes() {
        let mut w = veterans();
        w.units[0].health = 0.4;
        w.set_experience(0, 0.0);
        assert_eq!(w.units[0].health, 0.4);
    }

    #[test]
    fn members_away_from_the_map_are_rescaled_too() {
        let mut w = veterans();
        w.units[0].left = Some(crate::Left::Unpacked(0));
        w.units[0].health = 50.0;
        w.gain_experience(0, 10.0);
        assert_eq!(w.units[0].health, 75.0);
    }

    #[test]
    fn technologies_and_held_properties_add_to_fighting_too() {
        let mut w = veterans();
        let tough = CombatEffect {
            defense: 1.0,
            health: 1.0,
            ..Default::default()
        };
        w.rules.combat_effects.insert("tough".into(), tough);
        w.give(Piece::Organization(0), "tough".into(), 1.0);
        w.units[1].fighter.as_mut().unwrap().weapons.clear();
        w.units[0].health = 50.0;
        w.tick();
        assert_eq!((w.max_health(0), w.units[0].health), (200.0, 100.0));
        assert_eq!(w.effect(Target::Unit(0)).defense, 1.0);
        // Once it's gone, back again.
        run(&mut w, 3.0);
        assert_eq!(w.max_health(0), 100.0);
        assert_eq!(w.effect(Target::Unit(0)).defense, 0.0);
    }

    #[test]
    fn kills_are_worth_more_against_the_stronger() {
        let mut w = veterans();
        // An even kill is worth 1, and the last of an organization another
        // for the organization as a whole.
        w.units[1].die(0);
        kill(&mut w, 1, 0);
        assert!((w.organizations[0].experience - 2.0).abs() < 1e-4);
        // One twice as strong, half as much again.
        let mut w = veterans();
        rate(&mut w, "strong", 10.0);
        w.rules
            .combat_profiles
            .get_mut(&Id::from("strong"))
            .unwrap()
            .health = 400.0;
        w.units[1].template = "strong".into();
        let ratio = w.unit_value(1) / w.unit_value(0);
        assert!(ratio > 1.0);
        kill(&mut w, 1, 0);
        let gain = 0.5 + 0.5 * ratio;
        assert!((w.organizations[0].experience - gain).abs() < 1e-4);
        // Killing a friend is worth nothing.
        let mut w = veterans();
        w.organizations[1].owner = 1;
        kill(&mut w, 1, 0);
        assert_eq!(w.organizations[0].experience, 0.0);
    }

    #[test]
    fn a_kill_is_kept_within_the_gains_bounds() {
        let mut w = veterans();
        w.rules.experience_gains.gain_maximum = 0.8;
        kill(&mut w, 1, 0);
        assert!((w.organizations[0].experience - 0.8).abs() < 1e-4);
        // A victim worth nothing counts as −1, so is worth less than half.
        let mut w = veterans();
        w.units[1].template = "unrated".into();
        kill(&mut w, 1, 0);
        let gain = 0.5 - 0.5 / w.unit_value(0);
        assert!((w.organizations[0].experience - gain).abs() < 1e-4);
        // Unless it's faked, when it counts as the default value.
        let mut w = veterans();
        w.units[1].template = "unrated".into();
        let faked = Levels {
            levels: Vec::new(),
            fake_value: Some(50.0),
        };
        w.rules.experience_levels.insert("unrated".into(), faked);
        kill(&mut w, 1, 0);
        let gain = 0.5 + 0.5 / w.unit_value(0);
        assert!((w.organizations[0].experience - gain).abs() < 1e-4);
    }

    #[test]
    fn a_shot_that_kills_earns_the_killer() {
        let mut w = veterans();
        w.units[1].health = 0.05;
        run(&mut w, 1.0);
        assert!(!w.units[1].alive());
        // The unit and then its organization, at 1 each.
        assert!((w.organizations[0].experience - 2.0).abs() < 1e-3);
    }

    #[test]
    fn the_last_enemy_to_hit_within_the_timeout_is_credited() {
        // Dying of something else a second after an enemy's hit.
        let mut w = veterans();
        hit_by(&mut w, 1, 0);
        w.ticks += (1.0 / TICK) as u64;
        w.units[1].die(w.ticks);
        w.mourn(1);
        assert!((w.organizations[0].experience - 2.0).abs() < 1e-4);
        // Three seconds after, nobody.
        let mut w = veterans();
        hit_by(&mut w, 1, 0);
        w.ticks += (3.0 / TICK) as u64;
        w.units[1].die(w.ticks);
        w.mourn(1);
        assert_eq!(w.organizations[0].experience, 0.0);
        // Nor one that's no longer an enemy.
        let mut w = veterans();
        w.organizations[1].owner = 1;
        kill(&mut w, 1, 0);
        assert_eq!(w.organizations[0].experience, 0.0);
    }

    #[test]
    fn a_strikes_hit_takes_the_credit_from_whoever_hit_before() {
        let mut w = veterans();
        hit_by(&mut w, 1, 0);
        let struck = w.units[1].struck.map(|s| crate::Struck { from: None, ..s });
        (w.units[1].struck, w.organizations[1].struck) = (struck, struck);
        w.units[1].die(w.ticks);
        let morale = w.organizations[0].morale;
        w.mourn(1);
        assert_eq!(w.organizations[0].experience, 0.0);
        assert_eq!(w.organizations[0].morale, morale);
    }

    #[test]
    fn a_regiment_is_credited_to_the_last_to_hit_any_of_it() {
        let mut w = veterans();
        hit_by(&mut w, 1, 0);
        // Its last man dies with no recent hit of his own.
        w.units[1].struck = None;
        w.units[1].die(w.ticks);
        w.mourn(1);
        assert!((w.organizations[0].experience - 1.0).abs() < 1e-4);
    }

    #[test]
    fn staying_engaged_earns_a_second_at_a_time_after_the_first() {
        let mut w = veterans();
        for unit in &mut w.units {
            let weapon = &mut unit.fighter.as_mut().unwrap().weapons[0];
            let mut d = crate::testing::main_damage(weapon);
            d.amount = 0.0;
            weapon.hits = vec![tgmap::WeaponHit::main(d)];
        }
        run(&mut w, 1.0);
        assert!(!w.organizations[0].engaged.is_empty());
        assert_eq!(w.organizations[0].experience, 0.0);
        run(&mut w, 3.0);
        // An even match, 1 a second.
        let earned = w.organizations[0].experience;
        assert!((2.0..=3.0).contains(&earned), "{earned}");
        assert!((earned - earned.round()).abs() < 1e-4);
    }

    #[test]
    fn breaking_an_enemy_earns_those_engaging_it() {
        let mut w = veterans();
        run(&mut w, 0.5);
        assert!(!w.organizations[0].engaged.is_empty());
        w.rout(1);
        assert!((w.organizations[0].experience - 1.0).abs() < 1e-4);
    }

    #[test]
    fn organizations_placed_with_experience_start_at_its_level() {
        let mut w = veterans();
        let mut m = member(10, [30.0, 30.0], 2.0);
        m.fighter = Some(soldier(Vec::new()));
        let organization = tgmap::Organization {
            hq: None,
            id: 9,
            template: "squad".into(),
            owner: 1,
            facing: [0.0, 1.0],
            members: vec![m],
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
            experience: 25.0,
            efficiency: None,
            unpacks: None,
        };
        let r = w.push_organization(&organization, false);
        assert_eq!(w.organizations[r].level, 2);
        assert_eq!(w.units[w.organizations[r].units[0]].health, 200.0);
        assert!(levelled(&w).is_empty());
    }
}
