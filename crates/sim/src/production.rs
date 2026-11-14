//! Buildings' queues: putting out what they commission and sending it on.

use crate::{Commission, Notice, Work, World, deploy_point, normalised};

impl World {
    pub(crate) fn enqueue(&mut self, building: usize, mut commission: Commission) {
        commission.id = self.next_commission;
        self.next_commission += 1;
        self.buildings[building].queue.push_back(commission);
    }

    /// Calls off all a building's commissions, giving its owner back what
    /// they paid.
    pub(crate) fn drop_queue(&mut self, building: usize) {
        let b = &mut self.buildings[building];
        let refund: f32 = b.queue.drain(..).map(|c| c.paid).sum();
        b.progress = 0;
        b.paused = None;
        let owner = b.owner;
        self.refund(owner, refund);
    }

    pub(crate) fn refund(&mut self, player: u32, amount: f32) {
        if amount > 0.0 {
            let back = self.stockpile(player) + amount;
            self.set_stockpile(player, back);
        }
    }

    /// Moves buildings on with what they're commissioning, and puts out
    /// what's done where they deploy it, sent on to where it goes.
    pub(crate) fn produce(&mut self) {
        for b in 0..self.buildings.len() {
            if self.held_up(b) {
                continue;
            }
            let building = &mut self.buildings[b];
            let Some(first) = building.queue.front() else {
                continue;
            };
            building.progress += 1;
            if building.progress < first.ticks {
                continue;
            }
            building.progress = 0;
            let Some(done) = building.queue.pop_front() else {
                continue;
            };
            let mut regiment = match done.work {
                Work::Raise(regiment) => *regiment,
                Work::Research(item) => {
                    self.researched(b, &item);
                    continue;
                }
            };
            if let Some(to) = done.to
                && self.dispatch(b, regiment.clone(), to)
            {
                continue;
            }
            let building = &self.buildings[b];
            let Some(deploy) = building.deploy else {
                continue;
            };
            let (position, facing, owner) = (building.position, building.facing, building.owner);
            let rally = building.rally;
            let from = deploy_point(position, facing, deploy.creation);
            let to = deploy_point(position, facing, deploy.deployment);
            let heading = normalised([to[0] - from[0], to[1] - from[1]]).unwrap_or(facing);
            regiment.owner = owner;
            regiment.place(from, heading);
            let r = self.add_new_regiment(regiment);
            log::debug!(
                "{} {} put out by {} {}",
                self.regiments[r].template,
                self.regiments[r].id,
                self.buildings[b].template,
                self.buildings[b].id
            );
            self.regiments[r].hq = self.hq_of(b);
            self.notices.push(Notice::PutOut {
                player: owner,
                building: b,
                regiment: r,
            });
            // Those that resupply come out as their leader alone, the
            // rest to follow.
            let used = self.logistics_of(&self.regiments[r].template);
            if used.is_some_and(|l| l.supply_used) {
                let leader = self.regiments[r].leader.unwrap_or(0);
                for (p, u) in self.regiments[r].units.clone().into_iter().enumerate() {
                    // Only those resupply can bring.
                    let rate = self.rules.resupply_rates.get(&self.units[u].template);
                    let comes = rate.is_some_and(|&r| r > 0.0);
                    self.units[u].awaited = p != leader && comes;
                }
            }
            self.roll_out(r, to, heading);
            self.regiments[r].rally = rally;
        }
    }

    /// Moves regiments that have rolled out of what put them out on to its
    /// rally point.
    pub(crate) fn rally(&mut self) {
        for r in 0..self.regiments.len() {
            let Some(at) = self.regiments[r].rally else {
                continue;
            };
            if self.destroyed(r) {
                self.regiments[r].rally = None;
            } else if !self.underway(r) {
                let owner = self.regiments[r].owner;
                let _ = self.order_move(owner, r, at);
            }
        }
    }

    /// Sends a regiment just put out straight to `to`, through whatever
    /// put it out, each member to its place in the formation there.
    fn roll_out(&mut self, regiment: usize, to: [f32; 2], facing: [f32; 2]) {
        let r = &mut self.regiments[regiment];
        r.position = to;
        r.facing = facing;
        let offsets = &r.formations[r.current_formation()].offsets;
        for (&u, &offset) in r.units.iter().zip(offsets) {
            let unit = &mut self.units[u];
            if unit.awaited {
                continue;
            }
            unit.take(vec![aaa_map::from_local(to, facing, offset)], false);
            unit.end_facing = facing;
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use aaa_map::{Id, Requires};

    use crate::{Command, Notice, Refused, State, World};

    #[test]
    fn commissions_come_out_of_their_building_in_turn() {
        let mut w = headquarters();
        w.rules.recruit_times.insert(Id::from("truck"), 10.0);
        assert_eq!(
            w.commission(2, 0, &Id::from("truck"), None),
            Err(Refused::NotYours)
        );
        w.apply(1, commission(0, "truck")).unwrap();
        w.apply(1, commission(0, "truck")).unwrap();
        run(&mut w, 9.0);
        assert!(w.regiments.is_empty());
        run(&mut w, 1.0);
        assert_eq!(w.regiments.len(), 1);
        assert_eq!(w.buildings[0].queue.len(), 1);
        let put_out = Notice::PutOut {
            player: 1,
            building: 0,
            regiment: 0,
        };
        assert_eq!(w.notices.iter().filter(|&n| *n == put_out).count(), 1);
        let r = &w.regiments[0];
        assert_eq!(r.owner, 1);
        // New ids, the lone truck's its regiment's too.
        assert_eq!((r.id, w.units[0].id), (6, 6));
        // It drives out to the HQ's left, which facing -y is +x.
        assert!(near(w.units[0].position, [20.0, 20.0]));
        run(&mut w, 3.0);
        assert!(near(w.units[0].position, [26.0, 20.0]));
        // The first is out, so calling it off is refused; the second isn't.
        let cancel = |commission| Command::CancelCommission {
            building: 0,
            commission,
        };
        assert_eq!(w.apply(1, cancel(0)), Err(Refused::Unable));
        w.apply(1, cancel(1)).unwrap();
        run(&mut w, 10.0);
        assert_eq!(w.regiments.len(), 1);
    }

    #[test]
    fn commissions_need_their_owners_required_buildings() {
        let mut w = headquarters();
        let ids = |ids: &[&str]| ids.iter().map(|&id| Id::from(id)).collect();
        w.rules.requires.insert(
            Id::from("TRUCK"),
            Requires {
                structures: vec![ids(&["pool", "old_pool"]), ids(&["depot"])],
                ..Requires::default()
            },
        );
        let commission = || commission(0, "truck");
        let lacks = |w: &World| -> Vec<String> {
            w.lacks(1, 0, &truck())
                .structures
                .iter()
                .map(|id| id.to_string())
                .collect()
        };
        assert_eq!(lacks(&w), ["pool", "depot"]);
        assert_eq!(w.check(1, &commission()), Err(Refused::Lacking));
        assert_eq!(w.apply(1, commission()), Err(Refused::Lacking));
        // An enemy's doesn't count; one under a name it once had does.
        let mut theirs = building(6, 2, [5.0, 5.0]);
        theirs.template = "pool".into();
        let mut ours = building(7, 1, [35.0, 5.0]);
        ours.template = "Old_Pool".into();
        w.add_buildings(&[theirs, ours]);
        assert_eq!(lacks(&w), ["depot"]);
        w.rules.requires.get_mut(&Id::from("truck")).unwrap().any = true;
        assert!(lacks(&w).is_empty());
        w.apply(1, commission()).unwrap();
        // Losing it stops further commissions.
        w.buildings[2].state = State::Destroyed(0);
        assert_eq!(w.apply(1, commission()), Err(Refused::Lacking));
    }

    #[test]
    fn technologies_grey_or_hide_what_needs_them() {
        let mut w = headquarters();
        w.buildings[0].template = "hq".into();
        let needs = |properties: &[&str], hide: bool| Requires {
            properties: properties.iter().map(|&p| Id::from(p)).collect(),
            hide,
            ..Requires::default()
        };
        // A second member, which decides where the two differ.
        let mut regiment = truck();
        let mut rifleman = regiment.members[0].clone();
        rifleman.template = "rifleman".into();
        regiment.members.push(rifleman);
        recruits(&mut w, "hq", regiment.clone(), 1.0);
        let commission = || commission(0, "truck");
        w.rules
            .requires
            .insert(Id::from("truck"), needs(&["tech_heavy"], false));
        w.rules
            .requires
            .insert(Id::from("rifleman"), needs(&["boat"], true));
        let lacking = w.lacks(1, 0, &regiment);
        assert_eq!(
            lacking.properties,
            [Id::from("tech_heavy"), Id::from("boat")]
        );
        assert!(lacking.hidden);
        assert_eq!(w.apply(1, commission()), Err(Refused::Lacking));
        // What the recruiter carries itself counts only for it.
        w.rules.kinds.insert(
            Id::from("hq"),
            aaa_map::Kind {
                properties: vec![Id::from("boat")],
                ..aaa_map::Kind::default()
            },
        );
        let lacking = w.lacks(1, 0, &regiment);
        assert_eq!(lacking.properties, [Id::from("tech_heavy")]);
        assert!(!lacking.hidden);
        // A technology counts once the side holds it, and only for that side.
        w.grant(2, Id::from("tech_heavy"));
        assert!(!w.lacks(1, 0, &regiment).is_empty());
        w.grant(1, Id::from("tech_heavy"));
        assert!(w.lacks(1, 0, &regiment).is_empty());
        w.apply(1, commission()).unwrap();
    }

    #[test]
    fn commissions_put_out_onto_a_neighbours_ground_can_leave_it() {
        // A depot beside the HQ covers where it puts out what it makes,
        // and a house stands in the way out.
        let mut w = headquarters();
        let depot = building(6, 1, [28.0, 20.0]);
        let house = building(7, 1, [26.0, 12.0]);
        for b in [&depot, &house] {
            w.pathing.block(b.obstacle.as_ref().unwrap());
        }
        w.add_buildings(&[depot, house]);
        w.apply(1, commission(0, "truck")).unwrap();
        run(&mut w, 4.0);
        assert!(near(w.units[0].position, [26.0, 20.0]));
        w.apply(
            1,
            Command::Move {
                regiment: 0,
                to: [26.0, 4.0],
            },
        )
        .unwrap();
        for _ in 0..300 {
            w.tick();
            assert!(w.buildings[2].distance(w.units[0].position) > 0.0);
        }
        assert!(near(w.units[0].position, [26.0, 4.0]));
    }

    #[test]
    fn commissions_move_on_to_the_rally_point() {
        let mut w = headquarters();
        assert_eq!(
            w.set_rally(2, 0, Some([26.0, 30.0])),
            Err(Refused::NotYours)
        );
        w.apply(
            1,
            Command::Rally {
                building: 0,
                at: Some([26.0, 30.0]),
            },
        )
        .unwrap();
        w.apply(1, commission(0, "truck")).unwrap();
        run(&mut w, 1.0);
        // Out to its deployment point first, then on to the rally point.
        assert_eq!(w.destination(0), [26.0, 20.0]);
        run(&mut w, 3.0);
        assert_eq!(w.destination(0), [26.0, 30.0]);
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [26.0, 30.0]));
        assert!(w.regiments[0].rally.is_none());
        // An order on the way calls it off.
        w.apply(1, commission(0, "truck")).unwrap();
        run(&mut w, 2.0);
        w.apply(1, Command::Stop { regiment: 1 }).unwrap();
        run(&mut w, 8.0);
        assert!(w.units[1].position[1] < 25.0);
        // Without one, they stay where they come out.
        w.apply(
            1,
            Command::Rally {
                building: 0,
                at: None,
            },
        )
        .unwrap();
        w.apply(1, commission(0, "truck")).unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[2].position, [26.0, 20.0]));
        let mut house = siege(&[building(9, 1, [10.0, 30.0])]);
        assert_eq!(
            house.set_rally(1, 0, Some([0.0, 0.0])),
            Err(Refused::Unable)
        );
    }
}
