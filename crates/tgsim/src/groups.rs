//! Groups: regiments ordered as one with Group Behavior on, timed so that
//! those moving arrive together.

use crate::Piece;

use crate::supply::BEAT;
use crate::{Command, Refused, World, distance};

/// The least radius of a group's zone, which the regiments taking part in
/// its timing are inside.
const ZONE_RANGE_MIN: f32 = 18.0;

/// How near a building an order's point is to be on it.
const ON_BUILDING: f32 = 1.0;

/// The least speed a group hands a regiment; one it would hand less stands
/// and waits for the rest.
const SPEED_MIN: f32 = 1.0;

/// How far outside the supply zone of an enemy thing a group heads for its
/// regiments time their arrival at.
const SUPPLY_ZONE_BUFFER: f32 = 5.0;

/// Regiments ordered together as one.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub id: u32,
    pub owner: u32,
    /// Indices into [`World::organizations`], in the group's order.
    pub organizations: Vec<usize>,
    /// The building it was last sent onto, by index into
    /// [`World::buildings`].
    pub heading_for: Option<usize>,
    /// The orders queued for it as a whole, each its regiments' together,
    /// first first.
    pub queue: Vec<Vec<Command>>,
}

crate::digest!(Group {
    id,
    owner,
    organizations,
    heading_for,
    queue,
});

impl World {
    /// Gives `orders` to the regiments they're to as one group: a fresh
    /// group of the two or more that take them, each leaving any it was
    /// in, timed together from now on. Fewer than two take them each on
    /// their own. Refused only if every order is, or one is itself a
    /// group's.
    pub(crate) fn order_group(&mut self, player: u32, orders: Vec<Command>) -> Result<(), Refused> {
        if orders.iter().any(|o| matches!(o, Command::Group { .. })) {
            return Err(Refused::Unable);
        }
        let mut ordered: Vec<usize> = Vec::new();
        for o in orders.iter().filter_map(crate::commands::steered) {
            if self.commands(player, o) && !ordered.contains(&o) {
                ordered.push(o);
            }
        }
        let grouped = ordered.len() >= 2;
        let formed = grouped.then(|| {
            for &r in &ordered {
                self.leave_group(r);
            }
            let id = self.next_group;
            self.next_group += 1;
            let heading_for = self.heading_for(player, &orders);
            for &r in &ordered {
                self.organizations[r].group = Some(id);
            }
            self.groups.push(Group {
                id,
                owner: player,
                organizations: ordered,
                heading_for,
                queue: Vec::new(),
            });
            id
        });
        let mut done = Err(Refused::Unable);
        let mut refused = Vec::new();
        for order in orders {
            let r = crate::commands::steered(&order);
            match self.give_order(player, order, grouped) {
                Ok(()) => done = Ok(()),
                Err(e) => {
                    refused.extend(r);
                    if done.is_err() {
                        done = Err(e);
                    }
                }
            }
        }
        if let Some(id) = formed {
            // What didn't take its order isn't the group's.
            for r in refused {
                if self.organizations[r].group == Some(id) {
                    self.leave_group(r);
                }
            }
            self.mark_taking_part(id);
        }
        done
    }

    /// The enemy building `orders` send a group onto or at, by index into
    /// [`World::buildings`], whose supply zone it times its arrival by.
    fn heading_for(&self, player: u32, orders: &[Command]) -> Option<usize> {
        let enemy = |b: usize| {
            let owner = self.buildings[b].owner;
            self.sides.relation(player, owner) == crate::Relation::Enemy
                && self.buildings[b].standing()
        };
        let at = |to: [f32; 2]| {
            (0..self.buildings.len())
                .find(|&b| enemy(b) && self.buildings[b].distance(to) <= ON_BUILDING)
        };
        orders.iter().find_map(|o| match *o {
            Command::MoveOnto {
                piece: Piece::Building(b),
                ..
            }
            | Command::GuardPiece {
                piece: Piece::Building(b),
                ..
            } => enemy(b).then_some(b),
            Command::Attack { to, .. } | Command::Move { to, .. } | Command::Guard { to, .. } => {
                at(to)
            }
            _ => None,
        })
    }

    /// Takes the organization at `r` out of its group, which goes if
    /// that leaves it empty.
    pub(crate) fn leave_group(&mut self, r: usize) {
        let organization = &mut self.organizations[r];
        organization.group_speed = None;
        let Some(id) = organization.group.take() else {
            return;
        };
        if let Some(g) = self.groups.iter_mut().find(|g| g.id == id) {
            g.organizations.retain(|&o| o != r);
        }
        self.groups.retain(|g| !g.organizations.is_empty());
    }

    /// Marks those of the group `id`'s regiments inside its zone as
    /// taking part in its timing, and the rest as not: the circle round
    /// the average of their positions as wide as the widest control zone
    /// among them, and no narrower than [`ZONE_RANGE_MIN`].
    fn mark_taking_part(&mut self, id: u32) {
        let Some(g) = self.groups.iter().find(|g| g.id == id) else {
            return;
        };
        let centre = self.group_centre(&g.organizations);
        let reach = (g.organizations.iter())
            .map(|&r| self.organizations[r].control_range)
            .fold(ZONE_RANGE_MIN, f32::max);
        let inside: Vec<(usize, bool)> = (g.organizations.iter())
            .map(|&r| (r, distance(self.centre(r), centre) <= reach))
            .collect();
        for (r, inside) in inside {
            self.organizations[r].taking_part = inside;
        }
    }

    fn group_centre(&self, organizations: &[usize]) -> [f32; 2] {
        let n = organizations.len().max(1) as f32;
        (organizations.iter().map(|&r| self.centre(r)))
            .fold([0.0, 0.0], |[x, y], [a, b]| [x + a / n, y + b / n])
    }

    /// Once a second, for each group: takes out what's routing,
    /// exhausted, gone or no longer its owner's; and unless it's in a
    /// fight, which marks again who takes part and times nothing, hands
    /// each regiment taking part that's moving the speed that has it
    /// arrive when the furthest out in time does.
    pub(crate) fn keep_groups(&mut self) {
        let ticks = self.ticks;
        let leaving: Vec<usize> = (0..self.organizations.len())
            .filter(|&r| {
                let o = &self.organizations[r];
                let owner = self
                    .groups
                    .iter()
                    .find(|g| Some(g.id) == o.group)
                    .map(|g| g.owner);
                o.group.is_some()
                    && (o.routing()
                        || o.mind == crate::morale::Mind::Exhausted
                        || self.destroyed(r)
                        || owner != Some(o.owner))
            })
            .collect();
        for r in leaving {
            self.leave_group(r);
        }
        let ids: Vec<u32> = self.groups.iter().map(|g| g.id).collect();
        for id in ids {
            if !(ticks + u64::from(id)).is_multiple_of(BEAT) {
                continue;
            }
            if self.group_in_fight(id) {
                self.mark_taking_part(id);
                let g = self.groups.iter().find(|g| g.id == id).expect("listed");
                for r in g.organizations.clone() {
                    self.organizations[r].group_speed = None;
                }
                continue;
            }
            self.time_group(id);
        }
    }

    /// Whether the group `id` is in a fight: whether the first of its
    /// regiments taking part that is engaged is engaged with something
    /// other than what it's going after.
    fn group_in_fight(&self, id: u32) -> bool {
        let g = self.groups.iter().find(|g| g.id == id).expect("listed");
        let first = (g.organizations.iter())
            .map(|&r| &self.organizations[r])
            .find(|o| o.taking_part && !o.engaged.is_empty());
        first.is_some_and(|o| match o.order {
            crate::Order::Pursue(t) => !o.engaged.iter().any(|&p| self.piece_of(p, t)),
            _ => true,
        })
    }

    /// Whether `piece` is the organization at `t`.
    fn piece_of(&self, piece: Piece, t: usize) -> bool {
        piece == Piece::Organization(t)
    }

    /// Hands each of the group `id`'s regiments taking part and moving the
    /// speed that makes its time there the longest's: its own speed times
    /// its time over the longest. Heading for an enemy thing with a supply
    /// zone, each times its arrival at the zone's edge, a little out, less
    /// how far ahead of the group's centre it is.
    fn time_group(&mut self, id: u32) {
        let g = self
            .groups
            .iter()
            .find(|g| g.id == id)
            .expect("listed")
            .clone();
        let zone = g.heading_for.and_then(|b| {
            let range = self.supply_range(b)?;
            let enemy = self.buildings[b].owner != g.owner;
            enemy.then(|| (self.buildings[b].position, range))
        });
        let centre = self.group_centre(&g.organizations);
        let facing =
            zone.and_then(|(at, _)| crate::normalised([at[0] - centre[0], at[1] - centre[1]]));
        let places = facing.map(|f| self.group_places(&g.organizations, f));
        let times: Vec<(usize, f32, f32)> = (g.organizations.iter().enumerate())
            .filter(|&(_, &r)| self.organizations[r].taking_part && self.underway(r))
            .filter_map(|(i, &r)| {
                let speed = self.unhurried_speed(r);
                let lp = self.pacer(r)?;
                let mut left = self.units[self.organizations[r].units[lp]].route_left();
                if let (Some((_, range)), Some(places)) = (zone, &places) {
                    let ahead = places[i][1];
                    left = (left - (range + SUPPLY_ZONE_BUFFER + ahead)).max(0.0);
                }
                (speed > 0.0).then_some((r, speed, left / speed))
            })
            .collect();
        let longest = times.iter().map(|&(_, _, t)| t).fold(0.0, f32::max);
        for &r in &g.organizations {
            self.organizations[r].group_speed = None;
        }
        if longest <= 0.0 {
            return;
        }
        for (r, speed, time) in times {
            self.organizations[r].group_speed = Some(speed * time / longest);
        }
    }

    /// Each of `organizations`' places in their group facing `facing`,
    /// from its centre, x to its right and y ahead: all in one formation,
    /// the positions its group formation gives them in turn; otherwise
    /// where each stands, one not taking part put at the zone's least
    /// radius in its own direction. Either way set about the centre.
    fn group_places(&self, organizations: &[usize], facing: [f32; 2]) -> Vec<[f32; 2]> {
        let formation = |r: usize| {
            let o = &self.organizations[r];
            (!o.broken()).then(|| o.formations.get(o.current_formation()).map(|f| &f.id))?
        };
        let first = organizations.first().and_then(|&r| formation(r));
        let shared = first.filter(|&id| organizations.iter().all(|&r| formation(r) == Some(id)));
        let given = shared
            .and_then(|id| {
                self.rules
                    .group_formations
                    .get(&tgmap::Id::from(id.as_str()))
            })
            .filter(|positions| positions.len() >= organizations.len());
        let centre = self.group_centre(organizations);
        let places: Vec<[f32; 2]> = match given {
            Some(positions) => positions[..organizations.len()].to_vec(),
            None => (organizations.iter())
                .map(|&r| {
                    let at = tgmap::to_local(centre, facing, self.centre(r));
                    match self.organizations[r].taking_part {
                        true => at,
                        false => crate::normalised(at)
                            .map_or(at, |[x, y]| [x * ZONE_RANGE_MIN, y * ZONE_RANGE_MIN]),
                    }
                })
                .collect(),
        };
        let n = places.len().max(1) as f32;
        let [mx, my] = (places.iter()).fold([0.0, 0.0], |[x, y], [a, b]| [x + a / n, y + b / n]);
        places.iter().map(|[x, y]| [x - mx, y - my]).collect()
    }

    /// Whether the organization at `r` is waiting for the rest of its
    /// group: handed too low a speed to crawl at, it stands.
    pub(crate) fn waiting_for_group(&self, r: usize) -> bool {
        self.organizations[r]
            .group_speed
            .is_some_and(|s| s < SPEED_MIN)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::*;
    use crate::{Command, SECOND};

    use tgmap::{Domain, Pathing};

    /// Player 7's two squads of one man each, side by side.
    fn grouped() -> crate::World {
        let mut w = world(Pathing::uniform(60, Domain::Land));
        let mut other = w.organizations[0].clone();
        other.units = Vec::new();
        let second = w.organizations.len();
        w.organizations.push(other);
        // The slower man moves into a squad of his own.
        w.organizations[0].units = vec![0];
        w.organizations[second].units = vec![1];
        w.units[1].organization = second;
        w.organizations[0].leader = Some(0);
        w.organizations[second].leader = Some(0);
        for r in [0, second] {
            w.organizations[r].speed = 2.0;
        }
        w.units[0].speed_max = 2.0;
        w.units[1].speed_max = 2.0;
        w
    }

    fn moves(to: [[f32; 2]; 2]) -> Command {
        Command::Group {
            orders: vec![
                Command::Move {
                    organization: 0,
                    to: to[0],
                },
                Command::Move {
                    organization: 1,
                    to: to[1],
                },
            ],
        }
    }

    #[test]
    fn a_group_slows_its_nearer_regiments_to_arrive_with_the_furthest() {
        let mut w = grouped();
        let order = moves([[9.0, 50.0], [11.0, 30.0]]);
        w.apply(7, order).unwrap();
        assert_eq!(w.organizations[0].group, w.organizations[1].group);
        run(&mut w, 2.0);
        // Half as far to go, it goes at half the speed.
        let (near, far) = (
            w.organizations[1].group_speed,
            w.organizations[0].group_speed,
        );
        assert!((far.unwrap() - 2.0).abs() < 1e-3, "{far:?}");
        assert!((near.unwrap() - 1.0).abs() < 0.2, "{near:?}");
        run(&mut w, 25.0);
        let [a, b] = [w.units[0].position[1], w.units[1].position[1]];
        assert!(a > 40.0 && b > 25.0, "{a} {b}");
    }

    #[test]
    fn an_order_to_one_regiment_takes_it_out_of_its_group() {
        let mut w = grouped();
        let order = moves([[9.0, 50.0], [11.0, 30.0]]);
        w.apply(7, order).unwrap();
        w.apply(
            7,
            Command::Move {
                organization: 1,
                to: [11.0, 40.0],
            },
        )
        .unwrap();
        assert_eq!(w.organizations[1].group, None);
        // Left alone, the group goes.
        assert_eq!(w.organizations[0].group.is_some(), w.groups.len() == 1);
        w.ticks += SECOND;
    }

    #[test]
    fn a_group_order_one_regiment_takes_is_its_own() {
        let mut w = grouped();
        w.apply(7, moves([[9.0, 50.0], [11.0, 30.0]])).unwrap();
        let alone = Command::Group {
            orders: vec![Command::Move {
                organization: 1,
                to: [11.0, 40.0],
            }],
        };
        w.apply(7, alone).unwrap();
        assert_eq!(w.organizations[1].group, None);
        // Nor does a group's order nest another's.
        let nested = Command::Group {
            orders: vec![moves([[9.0, 50.0], [11.0, 30.0]])],
        };
        assert!(w.apply(7, nested).is_err());
    }

    #[test]
    fn a_regiment_handed_too_low_a_speed_waits() {
        let mut w = grouped();
        let order = moves([[9.0, 55.0], [11.0, 12.0]]);
        w.apply(7, order).unwrap();
        run(&mut w, 2.0);
        assert!(w.waiting_for_group(1));
        let at = w.units[1].position;
        run(&mut w, 1.0);
        assert_eq!(w.units[1].position, at);
    }

    #[test]
    fn regiments_all_in_one_formation_take_its_group_formations_places() {
        let mut w = grouped();
        let line = vec![[0.0, 6.0], [-13.5, 6.0], [13.5, 6.0]];
        w.rules.group_formations.insert("line".into(), line);
        let north = [0.0, 1.0];
        let places = w.group_places(&[0, 1], north);
        assert!(near(places[0], [6.75, 0.0]) && near(places[1], [-6.75, 0.0]));
        // In different formations, each keeps where it stands, set about
        // the centre: the first, at 9,10, 1 to the left.
        w.organizations[1].formation = 1;
        for r in [0, 1] {
            w.organizations[r].taking_part = true;
        }
        let places = w.group_places(&[0, 1], north);
        assert!(
            near(places[0], [-1.0, 0.0]) && near(places[1], [1.0, 0.0]),
            "{places:?}"
        );
    }
}
