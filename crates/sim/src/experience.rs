//! The experience a side's faction earns, and spending it on the faction's
//! special operations.

use aaa_map::{Id, Operation};

use crate::{LANDINGS_KEPT, Piece, Refused, SECOND, TICK, World};

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
        Ok(())
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
        let pathing = aaa_map::Pathing::uniform(8, aaa_map::Domain::Land);
        World::from_regiments(crate::Sides::default(), pathing, &[])
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
        let resupply = aaa_map::Operation::Enchant {
            targets: aaa_map::Targets::default(),
            area: 15.0,
            max_targets: 4,
            property: Id::from("property_resupply"),
            seconds: 30.0,
        };
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
        let reveal = aaa_map::Operation::Reveal { seconds: 10.0 };
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
