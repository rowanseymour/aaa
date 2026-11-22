//! Organizations exploring: heading on, again and again, for the ground
//! their side knows least about around the point they were sent to.

use std::f32::consts::TAU;

use tgmap::pathing::CELL_SIZE;
use tgmap::{FogSetting, Tile};

use crate::{Refused, Seen, World, distance, normalised, within};

/// How near the point an organization's sent to explore some member must
/// be able to stand for it to head there first, and how near any place it
/// weighs.
const STANDING: f32 = 2.0;

/// An organization's order to explore, kept until it's given another.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Explore {
    /// The point it explores around.
    pub point: [f32; 2],
    /// Where it's headed now.
    pub heading: Option<[f32; 2]>,
    /// Whether it's fought since it chose where to head, so chooses
    /// afresh once done.
    fought: bool,
}

crate::digest!(Explore {
    point,
    heading,
    fought,
});

/// A place an exploring organization might head for next, and how well
/// its side knows it.
struct Candidate {
    at: [f32; 2],
    seen: Seen,
    value: f32,
}

impl World {
    /// Whether the organization at `organization` can be sent to explore:
    /// a regiment, not a lone unit, with a member that can move, while the
    /// match has fog.
    pub fn explores(&self, organization: usize) -> bool {
        self.fog.setting != FogSetting::None
            && self.organizations[organization].unpacks.is_none()
            && !self.lone(organization)
            && self
                .survivors(organization)
                .any(|u| self.units[u].speed_max > 0.0)
    }

    pub(crate) fn may_explore(&self, player: u32, organization: usize) -> Result<(), Refused> {
        self.may_command(player, organization)?;
        match self.explores(organization) {
            true => Ok(()),
            false => Err(Refused::Unable),
        }
    }

    /// Has an organization explore around `point`, heading there first
    /// if it could stand near it.
    pub(crate) fn order_explore(
        &mut self,
        player: u32,
        organization: usize,
        point: [f32; 2],
    ) -> Result<(), Refused> {
        self.may_explore(player, organization)?;
        let size = self.pathing.size();
        let point = point.map(|c| c.clamp(1.0, size.next_down()));
        let first = self
            .could_stand(organization, point, STANDING)
            .then_some(point);
        let r = &mut self.organizations[organization];
        r.take_order(crate::Order::Move);
        r.explore = Some(Explore {
            point,
            heading: first,
            fought: false,
        });
        match (first, r.engaged.is_empty()) {
            (Some(to), true) => self.send_along(organization, to),
            (None, true) => self.explore_on(organization),
            // It chooses once done fighting.
            (_, false) => {}
        }
        Ok(())
    }

    /// Has each exploring organization that's done with where it was
    /// headed choose where next: once it's no longer moving, has come
    /// near enough, or has fought since. One its group's leader explores
    /// for goes where the leader does.
    pub(crate) fn keep_exploring(&mut self) {
        let close = self.rules.explore_rules.close_enough;
        for r in 0..self.organizations.len() {
            let Some(explore) = self.organizations[r].explore else {
                continue;
            };
            if self.destroyed(r) || self.organizations[r].arriving {
                continue;
            }
            if !self.organizations[r].engaged.is_empty() {
                if let Some(e) = &mut self.organizations[r].explore {
                    e.fought = true;
                }
                continue;
            }
            if let Some(leader) = self.led(r) {
                if explore.fought {
                    self.rejoin(r, leader);
                }
                continue;
            }
            let near = explore
                .heading
                .is_some_and(|to| within(self.centre(r), to, close));
            if explore.fought || near || !self.underway(r) {
                self.explore_on(r);
            }
        }
    }

    /// The leader of the organization at `r`'s group, if it's another
    /// that's exploring too, which chooses where they all go.
    fn led(&self, r: usize) -> Option<usize> {
        let id = self.organizations[r].group?;
        let group = self.groups.iter().find(|g| g.id == id)?;
        let &leader = group.organizations.first()?;
        (leader != r && self.organizations[leader].explore.is_some()).then_some(leader)
    }

    /// Sends the organization at `r`, done fighting, where its group's
    /// `leader` is headed, rather than on to where it was going before.
    fn rejoin(&mut self, r: usize, leader: usize) {
        let to = self.organizations[leader].explore.and_then(|e| e.heading);
        self.organizations[r].resume = None;
        if let Some(e) = &mut self.organizations[r].explore {
            e.fought = false;
            e.heading = to;
        }
        if let Some(to) = to {
            self.send_along(r, to);
        }
    }

    /// Has the organization at `r` choose where to explore next and head
    /// there, with any of its group it leads; with nowhere, it stands.
    fn explore_on(&mut self, r: usize) {
        let Some(explore) = self.organizations[r].explore else {
            return;
        };
        let next = self.next_to_explore(r, explore);
        let e = Explore {
            heading: next,
            fought: false,
            ..explore
        };
        self.organizations[r].explore = Some(e);
        self.organizations[r].resume = None;
        let Some(to) = next else {
            return;
        };
        self.send_along(r, to);
        let followers: Vec<usize> = (0..self.organizations.len())
            .filter(|&o| self.led(o) == Some(r))
            .collect();
        for o in followers {
            if self.organizations[o].engaged.is_empty() {
                if let Some(e) = &mut self.organizations[o].explore {
                    e.heading = Some(to);
                }
                self.send_along(o, to);
            }
        }
    }

    /// The best of the places the organization at `r` weighs: the less its
    /// side knows of one, the better, then the more it's worth.
    fn next_to_explore(&mut self, r: usize, explore: Explore) -> Option<[f32; 2]> {
        let rules = self.rules.explore_rules;
        let here = self.centre(r);
        let owner = self.organizations[r].owner;
        let mut places = Vec::new();
        for _ in 0..rules.around_itself {
            let way = self.chance() * TAU;
            let [near, far] = rules.radius;
            let reach = near + self.chance() * (far - near);
            let at = [
                here[0] + libm::cosf(way) * reach,
                here[1] + libm::sinf(way) * reach,
            ];
            places.push((at, rules.itself_value));
        }
        for _ in 0..rules.around_point {
            let way = self.chance() * TAU;
            if let Some(at) = self.fog_ahead(r, owner, explore.point, way) {
                places.push((at, rules.point_value));
            }
        }
        let mut best: Option<Candidate> = None;
        for (at, base) in places {
            if !self.on_the_map(at) || !self.could_stand(r, at, STANDING) {
                continue;
            }
            let seen = self.seen_with(&[owner], at);
            let same_way = match explore.heading {
                Some(last) => {
                    let a = normalised([at[0] - here[0], at[1] - here[1]]);
                    let b = normalised([last[0] - here[0], last[1] - here[1]]);
                    a.zip(b).map_or(0.0, |(a, b)| a[0] * b[0] + a[1] * b[1])
                }
                None => 0.0,
            };
            let finishing = match seen == Seen::Unexplored && self.finishes(owner, at) {
                true => rules.finishing,
                false => 0.0,
            };
            let value = base
                + rules.per_distance * distance(at, explore.point)
                + rules.same_way * same_way
                + finishing
                + self.chance() * rules.fuzz;
            let better = best
                .as_ref()
                .is_none_or(|b| seen < b.seen || (seen == b.seen && value > b.value));
            if better {
                best = Some(Candidate { at, seen, value });
            }
        }
        best.map(|b| b.at)
    }

    /// A place on from `point` along `way`, past the first ground `owner`
    /// has never seen that the organization at `r` could get to, as far
    /// into that ground as it reaches up to its rules' tiles into the fog;
    /// none if that way only reaches the map's edge.
    fn fog_ahead(&self, r: usize, owner: u32, point: [f32; 2], way: f32) -> Option<[f32; 2]> {
        let step = [libm::cosf(way) * CELL_SIZE, libm::sinf(way) * CELL_SIZE];
        let here = self.centre(r);
        let domains: Vec<tgmap::Domain> =
            (self.survivors(r)).map(|u| self.units[u].domain).collect();
        let along = |n: u32| [point[0] + step[0] * n as f32, point[1] + step[1] * n as f32];
        let unseen = |at| self.seen_with(&[owner], at) == Seen::Unexplored;
        let reachable = |at| domains.iter().any(|&d| self.pathing.joined(d, here, at));
        let mut n = 0;
        let found = loop {
            let at = along(n);
            if !self.on_the_map(at) {
                return None;
            }
            if unseen(at) && reachable(at) {
                break n;
            }
            n += 1;
        };
        let into = self.rules.explore_rules.into_fog;
        let last = (found..=found + into)
            .take_while(|&n| self.on_the_map(along(n)) && unseen(along(n)))
            .last()?;
        Some(along(last))
    }

    /// Whether the tile under `at` is one `owner` has never seen whose
    /// neighbours on the map it has all seen.
    fn finishes(&self, owner: u32, at: [f32; 2]) -> bool {
        let tile = Tile::at(at);
        let neighbours = (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)));
        neighbours
            .filter(|&d| d != (0, 0))
            .map(|(dx, dy)| tile.offset(dx, dy).centre())
            .filter(|&c| self.on_the_map(c))
            .all(|c| self.seen_with(&[owner], c) != Seen::Unexplored)
    }

    fn on_the_map(&self, at: [f32; 2]) -> bool {
        let size = self.pathing.size();
        at.iter().all(|c| (0.0..size).contains(c))
    }

    /// Whether some member of the organization at `r` could stand within
    /// `reach` of `at`: a tile its kind of ground, clear, has its centre
    /// that near, or `at` is on one.
    fn could_stand(&self, r: usize, at: [f32; 2], reach: f32) -> bool {
        let centre = Tile::at(at);
        let n = (reach / CELL_SIZE).ceil() as i32 + 1;
        let tiles = (-n..=n).flat_map(|dy| (-n..=n).map(move |dx| centre.offset(dx, dy)));
        let near: Vec<Tile> = tiles
            .filter(|&t| t == centre || within(t.centre(), at, reach))
            .collect();
        (self.survivors(r)).any(|u| {
            let domain = self.units[u].domain;
            near.iter().any(|&t| self.pathing.passable(t, domain))
        })
    }
}

#[cfg(test)]
mod tests {
    use tgmap::{Domain, FogSetting, Id, Pathing, Tile};

    use crate::testing::*;
    use crate::{Command, Refused, Seen, World};

    /// The two-soldier squad of player 7 at 10,10 on an 80 across map
    /// that sees 6 around it, under `fog`.
    fn explorer(fog: FogSetting) -> World {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        let sight = tgmap::Sight {
            range: 6.0,
            remote: 0.0,
        };
        w.rules.sights.insert(Id::from("squad"), sight);
        w.set_fog(fog);
        w
    }

    fn explore(w: &mut World, organization: usize, to: [f32; 2]) -> Result<(), Refused> {
        w.apply(7, Command::Explore { organization, to })
    }

    #[test]
    fn regiments_that_move_explore_while_the_match_has_fog() {
        let mut w = explorer(FogSetting::None);
        assert!(!w.explores(0));
        assert_eq!(explore(&mut w, 0, [30.0, 30.0]), Err(Refused::Unable));
        w.set_fog(FogSetting::Full);
        assert!(w.explores(0));
        assert_eq!(explore(&mut w, 0, [30.0, 30.0]), Ok(()));
        assert_eq!(w.activity(0), "mind_exploring");
        // Nor a lone unit.
        let mut lone = vehicle(0.0, 0.0, None);
        lone.units[0].template = "tank".into();
        lone.set_fog(FogSetting::Full);
        assert!(!lone.explores(0));
    }

    #[test]
    fn it_heads_for_the_point_then_on_into_ground_never_seen_and_never_stops() {
        let mut w = explorer(FogSetting::Full);
        explore(&mut w, 0, [30.0, 30.0]).unwrap();
        assert_eq!(
            w.organizations[0].explore.unwrap().heading,
            Some([30.0, 30.0])
        );
        run(&mut w, 15.0);
        let heading = w.organizations[0].explore.unwrap().heading;
        assert!(heading.is_some_and(|h| h != [30.0, 30.0]), "{heading:?}");
        let explored = |w: &World| {
            (w.view(7).iter())
                .filter(|&&s| s != Seen::Unexplored)
                .count()
        };
        let before = explored(&w);
        run(&mut w, 60.0);
        assert!(explored(&w) > before);
        assert!(w.organizations[0].explore.is_some());
        assert_eq!(w.activity(0), "mind_exploring");
    }

    #[test]
    fn it_prefers_ground_never_seen() {
        let mut w = explorer(FogSetting::Full);
        explore(&mut w, 0, [30.0, 30.0]).unwrap();
        run(&mut w, 15.0);
        for _ in 0..20 {
            let e = w.organizations[0].explore.unwrap();
            let next = w.next_to_explore(0, e).unwrap();
            assert_eq!(w.seen_with(&[7], next), Seen::Unexplored);
        }
    }

    #[test]
    fn where_no_member_could_stand_it_chooses_at_once() {
        let mut w = explorer(FogSetting::Full);
        let point = [30.0, 30.0];
        let tile = Tile::at(point);
        for dy in -2..=2 {
            for dx in -2..=2 {
                w.pathing.set(tile.offset(dx, dy), Domain::Naval);
            }
        }
        explore(&mut w, 0, point).unwrap();
        let heading = w.organizations[0].explore.unwrap().heading;
        assert!(heading.is_some_and(|h| h != point), "{heading:?}");
    }

    #[test]
    fn standing_with_nowhere_to_go_isnt_resting() {
        let mut w = explorer(FogSetting::Full);
        assert!(w.resting(0));
        w.organizations[0].explore = Some(super::Explore {
            point: [10.0, 10.0],
            heading: None,
            fought: false,
        });
        assert!(!w.resting(0));
        assert_eq!(w.state(0), None);
    }

    #[test]
    fn another_order_ends_it() {
        let mut w = explorer(FogSetting::Full);
        explore(&mut w, 0, [30.0, 30.0]).unwrap();
        let stop = Command::Stop { organization: 0 };
        w.apply(7, stop).unwrap();
        assert_eq!(w.organizations[0].explore, None);
        assert_ne!(w.activity(0), "mind_exploring");
    }

    #[test]
    fn a_group_goes_where_its_leader_chooses() {
        let mut w = explorer(FogSetting::Full);
        let second = tgmap::Organization {
            hq: None,
            id: 11,
            template: "squad".into(),
            owner: 7,
            facing: [0.0, 1.0],
            members: vec![member(12, [20.0, 10.0], 3.0)],
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
            experience: 0.0,
            efficiency: None,
            unpacks: None,
        };
        w.add_organization(&second);
        let orders = [(0, [30.0, 30.0]), (1, [40.0, 30.0])]
            .map(|(organization, to)| Command::Explore { organization, to })
            .to_vec();
        w.apply(7, Command::Group { orders }).unwrap();
        run(&mut w, 20.0);
        let heading = |r: usize| w.organizations[r].explore.unwrap().heading;
        assert!(heading(0).is_some());
        assert_eq!(heading(1), heading(0));
    }
}
