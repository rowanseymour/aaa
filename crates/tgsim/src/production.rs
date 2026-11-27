//! Buildings' queues: putting out what they commission and sending it on.

use crate::delivery::spot;
use crate::summons::rings_round;
use crate::{Commission, Notice, Work, World, bounding_radius, deploy_point, distance, normalised};

/// How far apart the rings round where a regiment or company comes out
/// are, in world units: the game's search goes out roughly a unit at a
/// time.
const RING_STEP: f32 = 1.0;

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
            let mut organization = match done.work {
                Work::Raise(organization) => *organization,
                Work::Research(item) => {
                    self.researched(b, &item);
                    continue;
                }
            };
            if let Some(to) = done.to
                && self.dispatch(b, organization.clone(), to)
            {
                continue;
            }
            let building = &self.buildings[b];
            let (position, facing, owner) = (building.position, building.facing, building.owner);
            let rally = building.rally;
            organization.owner = owner;
            // Without points to come out by, a lone unit is made only
            // where there's no room round the building, the game using its
            // search the wrong way round, and refunded otherwise.
            let lone =
                matches!(&organization.members[..], [m] if m.template == organization.template);
            if building.deploy.is_none() && lone {
                let shape = organization.members[0].shape;
                // Only things stand in the way, not the ground.
                let blocking = self.blocking();
                let clear = |&(p, f): &([f32; 2], [f32; 2])| {
                    let there = spot(p, f, shape);
                    blocking.iter().all(|o| !o.overlaps(&there))
                };
                if self.lone_spots(b, shape).iter().any(clear) {
                    self.refund(owner, done.paid);
                    continue;
                }
                organization.place(position, facing);
                let r = self.add_new_organization(organization);
                self.put_out(b, r, rally);
                continue;
            }
            // A regiment's put round the building.
            let Some(deploy) = building.deploy else {
                let template = organization.template.clone();
                match self.come_out(b, organization) {
                    Some(r) => self.put_out(b, r, rally),
                    None => {
                        self.refund(owner, done.paid);
                        let told = Notice::NotCreated {
                            player: owner,
                            template,
                        };
                        self.notices.push(told);
                    }
                }
                continue;
            };
            let from = deploy_point(position, facing, deploy.creation);
            let to = deploy_point(position, facing, deploy.deployment);
            let heading = normalised([to[0] - from[0], to[1] - from[1]]).unwrap_or(facing);
            organization.place(from, heading);
            let r = self.add_new_organization(organization);
            // Those that resupply come out as their leader alone, the
            // rest to follow, unless they're recruited whole.
            let used = self.logistics_of(&self.organizations[r].template);
            if used.is_some_and(|l| l.supply_used && !l.recruit_full) {
                let leader = self.organizations[r].leader.unwrap_or(0);
                for (p, u) in self.organizations[r].units.clone().into_iter().enumerate() {
                    // Only those resupply can bring.
                    let rate = self.rules.resupply_rates.get(&self.units[u].template);
                    let comes = rate.is_some_and(|&r| r > 0.0);
                    self.units[u].awaited = p != leader && comes;
                }
            }
            self.roll_out(r, to, heading);
            self.put_out(b, r, rally);
        }
    }

    /// The 33 spots looked at for room for a lone unit of `shape` by the
    /// building at `b`: its position, then four rings of eight round it
    /// and round a step from it along its facing and that turned 120° and
    /// 240° clockwise, each ring as far out as the building's radius and
    /// the unit's together. Each comes with the way the unit would face
    /// there: the building's at its position, away from it on the rings.
    fn lone_spots(&self, b: usize, shape: Option<tgmap::Shape>) -> Vec<([f32; 2], [f32; 2])> {
        let building = &self.buildings[b];
        let (at, facing) = (building.position, building.facing);
        let own = (building.obstacle.as_ref()).map_or(0.0, |o| bounding_radius(o.shape));
        let reach = own + shape.map_or(0.0, bounding_radius);
        let step = self.rules.recruit_search_step;
        let along = libm::atan2f(facing[1], facing[0]);
        let off = |turn: f32| {
            let angle = along - turn.to_radians();
            [
                at[0] + step * libm::cosf(angle),
                at[1] + step * libm::sinf(angle),
            ]
        };
        let centres = [at, off(0.0), off(120.0), off(240.0)];
        let rings = centres
            .into_iter()
            .flat_map(|c| rings_round(c, facing, reach, 1));
        let away = |p: [f32; 2]| {
            (
                p,
                normalised([p[0] - at[0], p[1] - at[1]]).unwrap_or(facing),
            )
        };
        std::iter::once((at, facing))
            .chain(rings.map(away))
            .collect()
    }

    /// Tells the side the organization at `r` is out of the building at
    /// `b`, under the building's HQ, and sends it on to `rally`.
    fn put_out(&mut self, b: usize, r: usize, rally: Option<[f32; 2]>) {
        log::debug!(
            "{} {} put out by {} {}",
            self.organizations[r].template,
            self.organizations[r].id,
            self.buildings[b].template,
            self.buildings[b].id
        );
        self.organizations[r].hq = self.hq_of(b);
        self.happen(crate::triggers::Happening::Recruited {
            player: self.buildings[b].owner,
            template: self.organizations[r].template.clone(),
        });
        self.notices.push(Notice::PutOut {
            player: self.buildings[b].owner,
            building: b,
            organization: r,
        });
        self.organizations[r].rally = rally;
    }

    /// Puts `organization` on the map by the building at `centre`, which
    /// has no points its recruits come out by, facing the way it does,
    /// with only its leader there and the rest awaited, returning its
    /// index; none if its leader has nowhere to stand.
    pub(crate) fn come_out(
        &mut self,
        centre: usize,
        mut organization: tgmap::Organization,
    ) -> Option<usize> {
        let b = &self.buildings[centre];
        let (at, facing) = (b.position, b.facing);
        let leader = organization.primary.unwrap_or(0);
        let shape = organization.members.get(leader)?.shape;
        // The centre is what it comes out of, so doesn't stand in its
        // leader's way at the start point.
        let start = std::iter::once(at);
        self.first_clear(start, facing, shape, &self.blocking_but(Some(centre)))?;
        let reach = b
            .obstacle
            .as_ref()
            .map_or(0.0, |o| bounding_radius(o.shape))
            + spread_of(&organization);
        let count = (reach / RING_STEP).ceil().max(1.0) as u32;
        let around = rings_round(at, facing, RING_STEP, count);
        // With nowhere clear round it, it's still made at the start point.
        let stand = (self.first_clear(around, facing, shape, &self.blocking())).unwrap_or(at);
        // The leader stands there, the company's position wherever that
        // puts it.
        let offset = (organization.formations.get(organization.formation))
            .and_then(|f| f.offsets.get(leader).copied())
            .unwrap_or([0.0, 0.0]);
        let shift = tgmap::from_local([0.0, 0.0], facing, offset);
        organization.place([stand[0] - shift[0], stand[1] - shift[1]], facing);
        let r = self.add_new_organization(organization);
        // Those that resupply come out as their leader alone, the rest to
        // follow, as from a building's queue.
        let template = &self.organizations[r].template;
        if self
            .logistics_of(template)
            .is_some_and(|l| l.supply_used && !l.recruit_full)
        {
            let leader = self.organizations[r].leader.unwrap_or(0);
            for (p, u) in self.organizations[r].units.clone().into_iter().enumerate() {
                let rate = self.rules.resupply_rates.get(&self.units[u].template);
                self.units[u].awaited = p != leader && rate.is_some_and(|&r| r > 0.0);
            }
        }
        Some(r)
    }

    /// Moves organizations that have rolled out of what put them out on to its
    /// rally point.
    pub(crate) fn rally(&mut self) {
        for r in 0..self.organizations.len() {
            let Some(at) = self.organizations[r].rally else {
                continue;
            };
            if self.destroyed(r) {
                self.organizations[r].rally = None;
            } else if !self.underway(r) {
                let owner = self.organizations[r].owner;
                let _ = self.order_move(owner, r, at);
            }
        }
    }

    /// Sends an organization just put out straight to `to`, through whatever
    /// put it out, each member to its place in the formation there.
    fn roll_out(&mut self, organization: usize, to: [f32; 2], facing: [f32; 2]) {
        let r = &mut self.organizations[organization];
        r.position = to;
        r.facing = facing;
        let offsets = &r.formations[r.current_formation()].offsets;
        for (&u, &offset) in r.units.iter().zip(offsets) {
            let unit = &mut self.units[u];
            if unit.awaited {
                continue;
            }
            // Walking out to its place, it has to get right there.
            unit.take(vec![tgmap::from_local(to, facing, offset)], true);
            unit.end_facing = facing;
        }
    }
}

/// How far an organization's members stand from its position, at most,
/// in its starting formation.
fn spread_of(organization: &tgmap::Organization) -> f32 {
    let offsets = (organization.formations.get(organization.formation))
        .map_or(&[][..], |f| f.offsets.as_slice());
    (offsets.iter())
        .map(|&o| distance(o, [0.0, 0.0]))
        .fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use tgmap::{Id, Requires};

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
        assert!(w.organizations.is_empty());
        run(&mut w, 1.0);
        assert_eq!(w.organizations.len(), 1);
        assert_eq!(w.buildings[0].queue.len(), 1);
        let put_out = Notice::PutOut {
            player: 1,
            building: 0,
            organization: 0,
        };
        assert_eq!(w.notices.iter().filter(|&n| *n == put_out).count(), 1);
        let r = &w.organizations[0];
        assert_eq!(r.owner, 1);
        // New ids, the lone truck's its organization's too.
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
        assert_eq!(w.organizations.len(), 1);
    }

    /// The economy's HQ with no points its recruits come out by, earning
    /// nothing, and recruiting a free-standing truck for 250 and a squad
    /// of two of them.
    fn no_way_out() -> World {
        let mut w = economy();
        w.buildings[0].deploy = None;
        w.rules.economies.remove(&Id::from("house"));
        let truck = w.rules.economies.get_mut(&Id::from("truck")).unwrap();
        truck.upkeep = Default::default();
        let mut squad = crate::testing::truck();
        squad.template = "squad".into();
        squad.members.push(squad.members[0].clone());
        squad.formations[0].offsets = vec![[0.0, 0.0], [2.0, 0.0]];
        squad.unpacks = None;
        recruits(&mut w, "house", squad, 1.0);
        w
    }

    /// Hems the HQ in with a rock wider than any search round it.
    fn hem_in(w: &mut World) {
        w.scenery.push(tgmap::Obstacle {
            position: [20.0, 20.0],
            facing: [0.0, -1.0],
            shape: tgmap::Shape::Circle { radius: 12.0 },
            soft: tgmap::Shape::Circle { radius: 12.0 },
        });
    }

    #[test]
    fn a_regiment_with_no_way_out_is_put_round_its_building() {
        let mut w = no_way_out();
        w.apply(1, commission(0, "squad")).unwrap();
        run(&mut w, 1.0);
        assert_eq!(w.organizations.len(), 1);
        let put_out = Notice::PutOut {
            player: 1,
            building: 0,
            organization: 0,
        };
        assert!(w.notices.contains(&put_out));
        // Clear of the HQ's 6 by 6, facing the way it does.
        let leader = &w.units[w.organizations[0].units[0]];
        assert!(crate::distance(leader.position, [20.0, 20.0]) >= 3.0);
        assert_eq!(w.organizations[0].facing, w.buildings[0].facing);
        // Paid for, less the little it's earned since.
        assert!(w.stockpile(1) < 1.0);
    }

    #[test]
    fn a_regiment_with_nowhere_to_stand_is_refunded() {
        let mut w = no_way_out();
        hem_in(&mut w);
        w.apply(1, commission(0, "squad")).unwrap();
        run(&mut w, 1.0);
        assert!(w.organizations.is_empty() && w.buildings[0].queue.is_empty());
        assert_eq!(w.stockpile(1), 500.0);
        assert!(w.notices.contains(&Notice::NotCreated {
            player: 1,
            template: Id::from("squad"),
        }));
    }

    #[test]
    fn a_lone_unit_isnt_kept_off_the_water_round_its_building() {
        let mut w = no_way_out();
        w.pathing = tgmap::Pathing::uniform(40, tgmap::Domain::Naval);
        w.apply(1, commission(0, "truck")).unwrap();
        run(&mut w, 1.0);
        assert!(w.organizations.is_empty());
        assert_eq!(w.stockpile(1), 500.0);
    }

    #[test]
    fn a_lone_unit_with_no_way_out_is_made_only_where_theres_no_room() {
        let mut w = no_way_out();
        w.apply(1, commission(0, "truck")).unwrap();
        run(&mut w, 1.0);
        // Room round the HQ: nothing's made, and nothing said.
        assert!(w.organizations.is_empty() && w.buildings[0].queue.is_empty());
        assert_eq!(w.stockpile(1), 500.0);
        assert!(
            !w.notices
                .iter()
                .any(|n| matches!(n, Notice::NotCreated { .. }))
        );
        // Hemmed in, it's made on the HQ, facing the way it does.
        hem_in(&mut w);
        w.apply(1, commission(0, "truck")).unwrap();
        run(&mut w, 1.0);
        assert_eq!(w.organizations.len(), 1);
        assert!(near(w.units[0].position, [20.0, 20.0]));
        assert_eq!(w.organizations[0].facing, w.buildings[0].facing);
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
        let mut organization = truck();
        let mut rifleman = organization.members[0].clone();
        rifleman.template = "rifleman".into();
        organization.members.push(rifleman);
        recruits(&mut w, "hq", organization.clone(), 1.0);
        let commission = || commission(0, "truck");
        w.rules
            .requires
            .insert(Id::from("truck"), needs(&["tech_heavy"], false));
        w.rules
            .requires
            .insert(Id::from("rifleman"), needs(&["boat"], true));
        let lacking = w.lacks(1, 0, &organization);
        assert_eq!(
            lacking.properties,
            [Id::from("tech_heavy"), Id::from("boat")]
        );
        assert!(lacking.hidden);
        assert_eq!(w.apply(1, commission()), Err(Refused::Lacking));
        // What the recruiter carries itself counts only for it.
        w.rules.kinds.insert(
            Id::from("hq"),
            tgmap::Kind {
                properties: vec![Id::from("boat")],
                ..tgmap::Kind::default()
            },
        );
        let lacking = w.lacks(1, 0, &organization);
        assert_eq!(lacking.properties, [Id::from("tech_heavy")]);
        assert!(!lacking.hidden);
        // A technology counts once the side holds it, and only for that side.
        w.grant(2, Id::from("tech_heavy"));
        assert!(!w.lacks(1, 0, &organization).is_empty());
        w.grant(1, Id::from("tech_heavy"));
        assert!(w.lacks(1, 0, &organization).is_empty());
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
                organization: 0,
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
        assert!(w.organizations[0].rally.is_none());
        // An order on the way calls it off.
        w.apply(1, commission(0, "truck")).unwrap();
        run(&mut w, 2.0);
        w.apply(1, Command::Stop { organization: 1 }).unwrap();
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
