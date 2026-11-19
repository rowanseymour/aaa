//! Special operations that give what they reach a property for a time,
//! and what those properties do to the units holding them while they last.

use tgmap::{CombatEffect, Enchantment, Id};

use crate::combat::{Hit, caught, damage, resistance};
use crate::strikes::Incoming;
use crate::supply::BEAT;
use crate::targets::{Thing, User};
use crate::{Held, Piece, Refused, Relation, TICK, Target, World, bounding_radius};

impl World {
    /// Carries out `player`'s enchantment `operation`, aimed at `at` or
    /// given on `on` there: at once, or once it lands for one that takes a
    /// while to. One given on a thing it would do nothing to is called off.
    pub(crate) fn enchant(
        &mut self,
        player: u32,
        operation: &Id,
        e: &Enchantment,
        at: [f32; 2],
        on: Option<Piece>,
    ) -> Result<(), Refused> {
        if on.is_some_and(|on| self.wasted_on(player, e, on)) {
            return Err(Refused::Unable);
        }
        if e.delay > 0.0 {
            self.incoming.push(Incoming {
                by: player,
                operation: operation.clone(),
                lands: self.ticks + (e.delay / TICK).round() as u64,
                at,
                on,
            });
        } else {
            let hits = self.take_effect(player, e, at);
            for hit in hits {
                self.land(hit);
            }
        }
        Ok(())
    }

    /// What of `on` `player`'s enchantment `e` works on: a regiment
    /// itself, or those of its living members it works on one by one; a
    /// building if it fits.
    fn enchanted_in(&self, player: u32, e: &Enchantment, on: Piece) -> Vec<Thing> {
        let user = User::side(player);
        let fits = |t: Thing| self.reaches(user, &e.targets, t);
        match on {
            Piece::Building(b) => {
                let standing = self.buildings.get(b).is_some_and(|b| b.standing());
                (standing && fits(Thing::Building(b))).then_some(Thing::Building(b))
            }
            .into_iter()
            .collect(),
            Piece::Organization(r) if fits(Thing::Organization(r)) => {
                vec![Thing::Organization(r)]
            }
            Piece::Organization(r) => (self.organizations[r].units.iter())
                .filter(|&&u| self.units[u].alive())
                .map(|&u| Thing::Unit(u))
                .filter(|&t| fits(t))
                .collect(),
        }
    }

    /// Whether `player`'s enchantment `e` can be given on `on`: on
    /// anything of it the enchantment works on.
    pub(crate) fn enchantable(&self, player: u32, e: &Enchantment, on: Piece) -> bool {
        !self.enchanted_in(player, e, on).is_empty()
    }

    /// Whether `player`'s enchantment `e` given straight on `on` would do
    /// nothing to it: it holds the property already, or it isn't
    /// friendly and keeps off all the enchantment does. Given on a
    /// regiment that it works on unit by unit, that's each of the members
    /// it works on.
    fn wasted_on(&self, player: u32, e: &Enchantment, on: Piece) -> bool {
        let things = self.enchanted_in(player, e, on);
        let only_property = e.morale == 0.0 && e.damage.is_empty();
        let wasted = |t: Thing| {
            let holds = self.timed(t).iter().any(|h| h.property == e.property);
            holds || only_property && self.keeps_off(player, e.kind.as_ref(), t)
        };
        !things.is_empty() && things.into_iter().all(wasted)
    }

    /// Does what `player`'s enchantment `e` does around `at`: its damage,
    /// its property and its morale to each thing it works on, returning
    /// the damage's hits to land.
    pub(crate) fn take_effect(&mut self, player: u32, e: &Enchantment, at: [f32; 2]) -> Vec<Hit> {
        let mut hits = Vec::new();
        for t in self.enchanted(player, e, at) {
            if let Some(target) = target_of(t) {
                let user = User::side(player);
                for (targets, d) in &e.damage {
                    let Some(f) = self.fighter(target) else {
                        continue;
                    };
                    if !self.reaches(user, targets, target) {
                        continue;
                    }
                    let shelter = self.shelter(target);
                    let ground = self.protection(target, &shelter);
                    let blow = self.blow(None, None, target);
                    let amount = damage(d, f, &ground, 1.0, blow);
                    hits.push(Hit::strike(target, amount, player, Some(d.kind.clone())));
                }
            }
            self.bestow(t, player, &e.property, e.seconds, e.kind.as_ref());
            let r = match t {
                Thing::Organization(r) => Some(r),
                Thing::Unit(u) => Some(self.units[u].organization),
                Thing::Building(_) => None,
            };
            if let Some(r) = r {
                match e.morale > 0.0 {
                    true => self.gain_morale(r, e.morale),
                    false => self.lose_morale(r, -e.morale, false),
                }
            }
        }
        hits
    }

    /// What `player`'s enchantment `e` used at `at` works on: whatever its
    /// targets match within its area, measured to each thing's edge, the
    /// nearest by their middles where it works on only so many.
    fn enchanted(&self, player: u32, e: &Enchantment, at: [f32; 2]) -> Vec<Thing> {
        let user = User::side(player);
        let regiments = (0..self.organizations.len())
            .filter(|&r| !self.destroyed(r))
            .map(|r| {
                let kind = self.rules.kinds.get(&self.organizations[r].template);
                let radius = kind.map_or(0.0, |k| bounding_radius(k.soft));
                (Thing::Organization(r), self.leader_position(r), radius)
            });
        let others = (self.soft_circles(self.targets())).map(|(t, at, r)| (Thing::from(t), at, r));
        let fits = |t| self.reaches(user, &e.targets, t);
        let caught = caught(at, e.area, e.max_targets, regiments.chain(others), fits);
        caught.into_iter().map(|(t, _)| t).collect()
    }

    /// Gives `t` `player`'s `property`, of enchantment type `kind`, for
    /// `seconds`, unless it holds it already or keeps it off. Any immunity
    /// the property brings clears what it keeps off from `t` at once.
    pub(crate) fn bestow(
        &mut self,
        t: Thing,
        player: u32,
        property: &Id,
        seconds: f32,
        kind: Option<&Id>,
    ) {
        if self.timed(t).iter().any(|h| &h.property == property) || self.keeps_off(player, kind, t)
        {
            return;
        }
        let until = self.ticks + (seconds / TICK).round() as u64;
        self.timed_mut(t).push(Held {
            property: property.clone(),
            until,
            by: Some(player),
        });
        let cleared: Vec<Id> = (self.timed(t).iter())
            .filter(|h| {
                let kind = self.rules.enchantment_types.get(&h.property);
                h.by.is_some_and(|by| self.keeps_off(by, kind, t))
            })
            .map(|h| h.property.clone())
            .collect();
        self.timed_mut(t).retain(|h| !cleared.contains(&h.property));
    }

    /// Whether `t` keeps off enchantments of type `kind` that `player`
    /// gives: one of a side that's neither its own nor an ally, that it's
    /// immune to.
    fn keeps_off(&self, player: u32, kind: Option<&Id>, t: Thing) -> bool {
        let owner = self.thing_owner(t);
        let friendly = matches!(
            self.sides.relation(player, owner),
            Relation::Itself | Relation::Ally
        );
        !friendly && kind.is_some_and(|k| self.thing_effect(t).immune_to(k))
    }

    /// What the properties `t` holds add to its fighting.
    fn thing_effect(&self, t: Thing) -> CombatEffect {
        match t {
            Thing::Organization(r) => {
                let o = &self.organizations[r];
                self.held_effect(o.owner, &o.template, &o.properties, [])
            }
            Thing::Unit(u) => self.boost(u),
            Thing::Building(b) => self.building_boost(b),
        }
    }

    /// The properties `t` holds for a time.
    fn timed(&self, t: Thing) -> &[Held] {
        match t {
            Thing::Organization(r) => &self.organizations[r].properties,
            Thing::Unit(u) => &self.units[u].properties,
            Thing::Building(b) => &self.buildings[b].properties,
        }
    }

    fn timed_mut(&mut self, t: Thing) -> &mut Vec<Held> {
        match t {
            Thing::Organization(r) => &mut self.organizations[r].properties,
            Thing::Unit(u) => &mut self.units[u].properties,
            Thing::Building(b) => &mut self.buildings[b].properties,
        }
    }

    /// Looks at what each thing holds for a time once a second, on its own
    /// second: what's run its time goes. Then each unit and building takes
    /// the damage a second of what it holds for a time and its side's
    /// technologies do it.
    pub(crate) fn keep_enchantments(&mut self) {
        let ticks = self.ticks;
        let due = |i: usize| (ticks + i as u64).is_multiple_of(BEAT);
        for r in (0..self.organizations.len()).filter(|&r| due(r)) {
            self.organizations[r].properties.retain(|h| h.until > ticks);
        }
        for b in (0..self.buildings.len()).filter(|&b| due(b)) {
            self.buildings[b].properties.retain(|h| h.until > ticks);
        }
        for u in 0..self.units.len() {
            if (ticks + u as u64).is_multiple_of(BEAT) {
                self.units[u].properties.retain(|h| h.until > ticks);
                self.suffer(Target::Unit(u));
            }
        }
        for b in 0..self.buildings.len() {
            if (ticks + b as u64).is_multiple_of(BEAT) {
                self.suffer(Target::Building(b));
            }
        }
    }

    /// Takes from `t` a second's damage of what bears on it past its
    /// template's own properties, times resistance, whatever its defence.
    fn suffer(&mut self, t: Target) {
        let Some(f) = self.fighter(t) else {
            return;
        };
        let table = &self.rules.combat_effects;
        let mut periodic = self
            .borne(table, |e| &e.requires, t)
            .flat_map(|e| &e.periodic);
        let Some(first) = periodic.next() else {
            return;
        };
        let shelter = self.shelter(t);
        let ground = self.protection(t, &shelter);
        let hurt = |d: &tgmap::Damage| d.amount * (1.0 - resistance(f, &ground, &d.kind));
        let amount = hurt(first) + periodic.map(hurt).sum::<f32>();
        if amount <= 0.0 {
            return;
        }
        let ticks = self.ticks;
        match t {
            Target::Unit(u) => {
                let unit = &mut self.units[u];
                unit.health -= amount;
                if unit.health <= 0.0 {
                    unit.die(ticks);
                    self.mourn(u);
                    self.lost_member(u);
                }
            }
            Target::Building(b) => {
                self.buildings[b].health -= amount;
                if self.buildings[b].health <= 0.0 {
                    self.fall(b, None);
                }
            }
        }
    }
}

fn target_of(t: Thing) -> Option<Target> {
    match t {
        Thing::Unit(u) => Some(Target::Unit(u)),
        Thing::Building(b) => Some(Target::Building(b)),
        Thing::Organization(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::*;
    use crate::{Piece, Refused, Target, World};
    use tgmap::{CombatEffect, Damage, Enchantment, Id, Operation, Targets};

    fn targets(required: &[&str], excluded: &[&str]) -> Targets {
        Targets {
            required: required.iter().map(|&t| Id::from(t)).collect(),
            excluded: excluded.iter().map(|&t| Id::from(t)).collect(),
        }
    }

    /// What player 1's units get from player 1's Courage, or player 2's
    /// from player 2's Fear: an enchantment of the units of the side's
    /// friends or enemies, not regiments, `area` round where it's aimed.
    fn enchantment(property: &str, friend: bool, morale: f32) -> Enchantment {
        let side = if friend { "FRIEND" } else { "ENEMY" };
        Enchantment {
            targets: targets(&[side], &["ORGANIZATION", "BUILDING"]),
            area: 12.0,
            max_targets: 0,
            property: Id::from(property),
            seconds: 10.0,
            kind: Some(Id::from(property)),
            morale,
            damage: Vec::new(),
            delay: 0.0,
        }
    }

    /// Player 1's soldiers at 10,10 and 10,20 and player 2's at 10,30,
    /// none armed, their regiments at half of 100 morale, with Courage
    /// (+2 DV, immune to Fear) and Fear (attacks slower) to use.
    fn enchanted() -> World {
        let soldiers = vec![
            soldier(Vec::new()),
            soldier(Vec::new()),
            soldier(Vec::new()),
        ];
        let mut w = battle(&[1, 1, 2], soldiers, at_war());
        for r in &mut w.organizations {
            (r.morale_max, r.morale) = (100.0, 50.0);
        }
        let courage = CombatEffect {
            defense: 2.0,
            immune: vec![Some(Id::from("fear"))],
            ..Default::default()
        };
        let fear = CombatEffect {
            recharge: -0.3,
            ..Default::default()
        };
        let effects = &mut w.rules.combat_effects;
        effects.insert(Id::from("courage"), courage);
        effects.insert(Id::from("fear"), fear);
        let types = &mut w.rules.enchantment_types;
        types.insert(Id::from("courage"), Id::from("courage"));
        types.insert(Id::from("fear"), Id::from("fear"));
        let ops = &mut w.rules.operations;
        ops.insert(
            Id::from("courage"),
            Operation::Enchant(enchantment("courage", true, 1.0)),
        );
        ops.insert(
            Id::from("fear"),
            Operation::Enchant(enchantment("fear", false, -3.0)),
        );
        w
    }

    fn holds(w: &World, u: usize, property: &str) -> bool {
        w.units[u]
            .properties
            .iter()
            .any(|h| h.property.as_str() == property)
    }

    #[test]
    fn enchantments_give_each_unit_they_reach_its_own_property_for_a_time() {
        let mut w = enchanted();
        let courage = Id::from("courage");
        w.operate(1, &courage, [10.0, 12.0], None).unwrap();
        // Friendly units in reach, not their regiments nor the enemy.
        assert!(holds(&w, 0, "courage") && holds(&w, 1, "courage") && !holds(&w, 2, "courage"));
        assert!(w.organizations.iter().all(|r| r.properties.is_empty()));
        assert_eq!(w.defense(Target::Unit(0)), Some((2.0, 0.0)));
        // Each unit reached moves its own regiment's morale.
        let morale: Vec<f32> = w.organizations.iter().map(|r| r.morale).collect();
        assert_eq!(morale, [51.0, 51.0, 50.0]);
        // Again on those holding it gives no more time, but the morale
        // still comes.
        run(&mut w, 5.0);
        let morale = w.organizations[0].morale;
        w.operate(1, &courage, [10.0, 12.0], None).unwrap();
        assert_eq!(w.organizations[0].morale, morale + 1.0);
        run(&mut w, 5.5);
        assert!(!holds(&w, 0, "courage") && w.defense(Target::Unit(0)) == Some((0.0, 0.0)));
    }

    #[test]
    fn immunity_keeps_off_and_clears_an_enemys_enchantments_but_not_its_morale() {
        let mut w = enchanted();
        let fear = Id::from("fear");
        let courage = Id::from("courage");
        w.operate(2, &fear, [10.0, 10.0], None).unwrap();
        assert!(holds(&w, 0, "fear") && holds(&w, 1, "fear"));
        assert_eq!(w.fire_rate(Target::Unit(0)), 0.7);
        // Courage on the first clears it there.
        w.operate(1, &courage, [10.0, 0.0], None).unwrap();
        assert!(holds(&w, 0, "courage") && !holds(&w, 0, "fear"));
        assert!(holds(&w, 1, "fear"));
        // And keeps it off, though the morale still goes.
        w.operate(2, &fear, [10.0, 10.0], None).unwrap();
        assert!(!holds(&w, 0, "fear"));
        assert_eq!(w.organizations[0].morale, 51.0 - 6.0);
    }

    #[test]
    fn an_enchantment_given_on_what_it_would_do_nothing_to_is_called_off() {
        let mut w = enchanted();
        let courage = Id::from("courage");
        let on = Some(Piece::Organization(0));
        w.operate(1, &courage, [10.0, 0.0], on).unwrap();
        assert_eq!(
            w.operate(1, &courage, [10.0, 0.0], on),
            Err(Refused::Unable)
        );
        assert_eq!(w.organizations[0].morale, 51.0);
        // An operation of nothing but its property, on an enemy immune to
        // it, as smoke on the brave.
        let mut smoke = enchantment("fear", false, 0.0);
        smoke.property = Id::from("smoke");
        let smoke = Operation::Enchant(smoke);
        w.rules.operations.insert(Id::from("smoke"), smoke);
        assert_eq!(
            w.operate(2, &Id::from("smoke"), [10.0, 10.0], on),
            Err(Refused::Unable)
        );
        // On the ground beside it works.
        w.operate(2, &Id::from("smoke"), [10.0, 10.0], None)
            .unwrap();
        assert!(holds(&w, 1, "smoke") && !holds(&w, 0, "smoke"));
    }

    #[test]
    fn enchantments_hurt_what_their_damage_is_for_and_may_land_later() {
        let mut w = enchanted();
        w.units[0]
            .fighter
            .as_mut()
            .unwrap()
            .flags
            .push(Id::from("officer"));
        let mut purge = enchantment("courage", true, 1.0);
        let gunfire = Damage {
            kind: Id::from("gunfire"),
            amount: 800.0,
        };
        purge.damage = vec![(targets(&["officer"], &[]), gunfire)];
        purge.delay = 1.0;
        w.rules
            .operations
            .insert(Id::from("purge"), Operation::Enchant(purge));
        w.operate(1, &Id::from("purge"), [10.0, 12.0], None)
            .unwrap();
        assert!(w.units[0].alive() && !holds(&w, 1, "courage"));
        run(&mut w, 1.1);
        // Its own side's officer dies to it, credited to no one.
        assert!(!w.units[0].alive() && w.units[0].struck.is_none());
        assert!(w.units[1].alive() && holds(&w, 1, "courage"));
    }

    #[test]
    fn properties_that_hurt_do_so_each_second_through_resistance() {
        let mut w = enchanted();
        let march = CombatEffect {
            periodic: vec![Damage {
                kind: Id::from("flame"),
                amount: 10.0,
            }],
            ..Default::default()
        };
        let bags = CombatEffect {
            resists: vec![(Id::from("flame"), 0.5)],
            ..Default::default()
        };
        w.rules.combat_effects.insert(Id::from("march"), march);
        w.rules.combat_effects.insert(Id::from("bags"), bags);
        let mut march = enchantment("march", true, 0.0);
        march.seconds = 3.0;
        w.rules
            .operations
            .insert(Id::from("march"), Operation::Enchant(march));
        w.operate(1, &Id::from("march"), [10.0, 10.0], None)
            .unwrap();
        w.units[1].properties.push(crate::Held {
            property: Id::from("bags"),
            until: u64::MAX,
            by: None,
        });
        run(&mut w, 5.0);
        // Three seconds' worth, halved for the second.
        assert_eq!((w.units[0].health, w.units[1].health), (70.0, 85.0));
    }

    #[test]
    fn on_a_mixed_regiment_only_the_members_it_works_on_count() {
        let mut w = enchanted();
        // The second soldier is a tank in the first's regiment.
        w.units[1].fighter.as_mut().unwrap().flags = vec![Id::from("tank")];
        w.units[1].organization = 0;
        w.organizations[0].units.push(1);
        w.organizations[1].units.clear();
        let mut blitz = enchantment("blitz", true, 0.0);
        blitz.targets = targets(&["FRIEND", "tank"], &["ORGANIZATION"]);
        w.rules
            .operations
            .insert(Id::from("blitz"), Operation::Enchant(blitz));
        let on = Some(Piece::Organization(0));
        w.operate(1, &Id::from("blitz"), [10.0, 15.0], on).unwrap();
        assert!(holds(&w, 1, "blitz") && !holds(&w, 0, "blitz"));
        // Its infantry never will, so the tank holding it is enough.
        assert_eq!(
            w.operate(1, &Id::from("blitz"), [10.0, 15.0], on),
            Err(Refused::Unable)
        );
    }

    #[test]
    fn a_click_gives_an_enchantment_on_a_building_it_works_on() {
        let w = enchanted();
        let mut w = with_buildings(
            w,
            &[building(20, 1, [30.0, 10.0]), building(21, 2, [30.0, 30.0])],
        );
        let mut bags = enchantment("bags", true, 0.0);
        bags.targets = targets(&["FRIEND", "BUILDING"], &[]);
        w.rules
            .operations
            .insert(Id::from("bags"), Operation::Enchant(bags));
        w.lead(
            1,
            0.0,
            None,
            vec![crate::SpecialOp::new(Id::from("bags"), 0.0, 0.0)],
        );
        let at = [5.0, 5.0];
        assert_eq!(
            w.operation_aim(1, 0, Some(Piece::Building(0)), at),
            Some([30.0, 10.0])
        );
        assert_eq!(w.operation_aim(1, 0, Some(Piece::Building(1)), at), None);
        assert_eq!(w.operation_aim(1, 0, Some(Piece::Building(9)), at), None);
        let on = Some(Piece::Building(0));
        w.operate(1, &Id::from("bags"), [30.0, 10.0], on).unwrap();
        assert_eq!(w.buildings[0].properties.len(), 1);
        assert!(w.buildings[1].properties.is_empty());
        assert_eq!(
            w.operate(1, &Id::from("bags"), [30.0, 10.0], on),
            Err(Refused::Unable)
        );
    }

    #[test]
    fn one_that_lands_later_does_nothing_on_what_no_longer_fits_it() {
        let cast = |turned: bool| {
            let mut w = enchanted();
            let mut fear = enchantment("fear", false, 0.0);
            fear.delay = 1.0;
            w.rules
                .operations
                .insert(Id::from("fear"), Operation::Enchant(fear));
            let on = Some(Piece::Organization(2));
            w.operate(1, &Id::from("fear"), [10.0, 30.0], on).unwrap();
            if turned {
                w.organizations[2].owner = 1;
            }
            run(&mut w, 1.1);
            holds(&w, 2, "fear")
        };
        assert!(cast(false));
        assert!(!cast(true));
    }

    #[test]
    fn what_a_regiment_holds_hurts_each_of_its_members() {
        let mut w = enchanted();
        let march = CombatEffect {
            periodic: vec![Damage {
                kind: Id::from("flame"),
                amount: 10.0,
            }],
            ..Default::default()
        };
        w.rules.combat_effects.insert(Id::from("march"), march);
        w.give(Piece::Organization(0), Id::from("march"), 2.5);
        run(&mut w, 4.0);
        // For its 2.5 seconds: at once, and twice more.
        assert_eq!(w.units[0].health, 70.0);
        assert_eq!(w.units[1].health, 100.0);
    }
}
