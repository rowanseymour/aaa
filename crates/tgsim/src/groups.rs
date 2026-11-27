//! Groups: regiments ordered as one with Group Behavior on, timed so that
//! those moving arrive together.

use tgmap::Tile;
use tgmap::pathing::CELL_SIZE;

use crate::Piece;
use crate::supply::BEAT;
use crate::{Command, Refused, World, distance, within};

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
    /// The order it's carrying out, as each of its regiments took it on
    /// the order's own point; none once it's done.
    pub orders: Vec<Command>,
    /// The way it faces on the map.
    pub facing: [f32; 2],
    /// Each regiment's place from its centre, in its order: sideways,
    /// then ahead.
    pub places: Vec<[f32; 2]>,
    /// Where it was put when it last finished an order.
    pub at: Option<[f32; 2]>,
    /// Whether its regiments were all in one formation when it last
    /// looked.
    pub shared: bool,
    /// While it's in a fight, the tick it next sees to it.
    pub fight: Option<u64>,
    /// Whether it times every regiment that's moving, taking part or
    /// not, as a computer player's does.
    pub times_all: bool,
}

crate::digest!(Group {
    id,
    owner,
    organizations,
    heading_for,
    queue,
    orders,
    facing,
    places,
    at,
    shared,
    fight,
    times_all,
});

/// The way a fresh group faces.
const FACING: [f32; 2] = [0.0, -1.0];

/// `place`, in a group's frame, laid onto the map for a group facing
/// `facing`.
fn on_map([x, y]: [f32; 2], [fx, fy]: [f32; 2]) -> [f32; 2] {
    [x * fy + y * fx, -x * fx + y * fy]
}

/// The point an order is on, if it's one a group's regiments head for
/// their places round, and whether it sets which way the group faces.
fn order_point(world: &World, command: &Command) -> Option<([f32; 2], bool)> {
    match *command {
        Command::Move { to, .. } | Command::Guard { to, .. } | Command::Run { to, .. } => {
            Some((to, true))
        }
        Command::Attack { to, .. } => Some((to, false)),
        Command::MoveOnto { piece, .. } | Command::GuardPiece { piece, .. } => {
            Some((world.middle(piece), true))
        }
        _ => None,
    }
}

/// `command` sent on to `to` instead, if it's an order on a place a
/// group's regiments head for their places round.
fn aimed(command: &Command, aim: [f32; 2]) -> Option<Command> {
    let mut command = command.clone();
    match &mut command {
        Command::Move { to, .. }
        | Command::Guard { to, .. }
        | Command::Run { to, .. }
        | Command::Attack { to, .. } => *to = aim,
        _ => return None,
    }
    Some(command)
}

/// The thing `command` is on, if any.
fn order_piece(command: &Command) -> Option<Piece> {
    match *command {
        Command::MoveOnto { piece, .. }
        | Command::GuardPiece { piece, .. }
        | Command::Repair { piece, .. } => Some(piece),
        Command::Pursue { enemy, .. } => Some(Piece::Organization(enemy)),
        _ => None,
    }
}

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
        for o in orders.iter().filter_map(crate::commands::ordered) {
            if self.commands(player, o) && !ordered.contains(&o) {
                ordered.push(o);
            }
        }
        if ordered.len() < 2 {
            return self.each_alone(player, orders);
        }
        for &r in &ordered {
            self.leave_group(r);
        }
        let id = self.next_group;
        self.next_group += 1;
        for &r in &ordered {
            self.organizations[r].group = Some(id);
            self.organizations[r].taking_part = true;
        }
        let shared = self.shared_formation(&ordered).is_some();
        let times_all = self.sides.computers.contains_key(&player);
        self.groups.push(Group {
            id,
            owner: player,
            places: vec![[0.0, 0.0]; ordered.len()],
            organizations: ordered,
            heading_for: None,
            queue: Vec::new(),
            orders: Vec::new(),
            facing: FACING,
            at: None,
            shared,
            fight: None,
            times_all,
        });
        let (done, refused) = self.take_up(id, orders);
        // What didn't take its order isn't the group's.
        for r in refused {
            if self.organizations[r].group == Some(id) {
                self.leave_group(r);
            }
        }
        done
    }

    /// Gives each of `orders` on its own.
    fn each_alone(&mut self, player: u32, orders: Vec<Command>) -> Result<(), Refused> {
        let mut done = Err(Refused::Unable);
        for order in orders {
            match self.give_order(player, order, false) {
                Ok(()) => done = Ok(()),
                Err(e) if done.is_err() => done = Err(e),
                Err(_) => {}
            }
        }
        done
    }

    /// Has the group `id` take up `orders` as its order: facing its point,
    /// its places worked out afresh, each regiment sent for its place round
    /// the point. Returns whether any took it, and those that refused.
    fn take_up(&mut self, id: u32, orders: Vec<Command>) -> (Result<(), Refused>, Vec<usize>) {
        let player = self.group(id).owner;
        let heading_for = self.heading_for(player, &orders);
        let point = orders.iter().find_map(|o| order_point(self, o));
        let members = self.group(id).organizations.clone();
        let centre = (self.group(id).at).unwrap_or_else(|| self.group_centre(&members));
        let g = self.group_mut(id);
        g.heading_for = heading_for;
        g.at = None;
        g.fight = None;
        if let Some(f) = point
            .filter(|&(_, faces)| faces)
            .and_then(|(at, _)| crate::normalised([at[0] - centre[0], at[1] - centre[1]]))
        {
            g.facing = f;
        }
        self.mark_taking_part(id);
        self.work_out_places(id);
        let mut done = Err(Refused::Unable);
        let mut refused = Vec::new();
        let mut taken = Vec::new();
        for order in orders {
            let r = crate::commands::ordered(&order);
            let given = self.aim_for(id, &order).unwrap_or_else(|| order.clone());
            match self.give_order(player, given, true) {
                Ok(()) => {
                    done = Ok(());
                    taken.push(order);
                }
                Err(e) => {
                    refused.extend(r);
                    if done.is_err() {
                        done = Err(e);
                    }
                }
            }
        }
        self.group_mut(id).orders = taken;
        (done, refused)
    }

    /// Has the group `id` take up `orders` it had queued.
    pub(crate) fn take_up_queued(&mut self, id: u32, orders: Vec<Command>) -> Result<(), Refused> {
        self.take_up(id, orders).0
    }

    pub(crate) fn group(&self, id: u32) -> &Group {
        self.groups.iter().find(|g| g.id == id).expect("a group")
    }

    fn group_mut(&mut self, id: u32) -> &mut Group {
        self.groups
            .iter_mut()
            .find(|g| g.id == id)
            .expect("a group")
    }

    /// `order` to one of the group `id`'s regiments, sent for its place
    /// round the order's point instead, if it's on a place.
    fn aim_for(&self, id: u32, order: &Command) -> Option<Command> {
        let r = crate::commands::ordered(order)?;
        let (point, _) = order_point(self, order)?;
        aimed(order, self.place_round(id, r, point)?)
    }

    /// Where the regiment at `r` of the group `id` stands round `point`:
    /// its place laid onto the map as the group faces, pulled in to the
    /// first tile centre inside the circle of the group's zone radius.
    fn place_round(&self, id: u32, r: usize, point: [f32; 2]) -> Option<[f32; 2]> {
        let g = self.group(id);
        let i = g.organizations.iter().position(|&o| o == r)?;
        let [dx, dy] = on_map(*g.places.get(i)?, g.facing);
        let at = [point[0] + dx, point[1] + dy];
        let reach = self.zone_radius(&g.organizations);
        let size = self.pathing.size();
        let on_map = |[x, y]: [f32; 2]| {
            [
                x.clamp(0.0, size.next_down()),
                y.clamp(0.0, size.next_down()),
            ]
        };
        if within(at, point, reach) {
            return Some(on_map(at));
        }
        let length = distance(at, point);
        let step = CELL_SIZE / 2.0;
        let pulled = (0..=(length / step).ceil() as u32)
            .map(|n| {
                let t = (n as f32 * step / length).min(1.0);
                Tile::at([
                    at[0] + (point[0] - at[0]) * t,
                    at[1] + (point[1] - at[1]) * t,
                ])
                .centre()
            })
            .find(|&c| within(c, point, reach))
            .unwrap_or(point);
        Some(on_map(pulled))
    }

    /// Works out the group `id`'s places afresh, as it faces.
    fn work_out_places(&mut self, id: u32) {
        let g = self.group(id);
        let places = self.group_places(&g.organizations, g.facing);
        self.group_mut(id).places = places;
    }

    /// Gives the group `id`'s order to its regiments again, each planning
    /// afresh for its place round the order's point, what they have
    /// queued kept, its places turned to the way it faces now. An order
    /// to move onto or guard a thing that's gone is dropped instead.
    fn order_again(&mut self, id: u32) {
        let g = self.group(id).clone();
        let gone = (g.orders.iter())
            .filter(|o| matches!(o, Command::MoveOnto { .. } | Command::GuardPiece { .. }))
            .filter_map(order_piece)
            .any(|p| self.piece_gone(p));
        if gone {
            self.finish_order(id, None);
            return;
        }
        let centre = self.group_centre(&g.organizations);
        if let Some(f) = (g.orders.iter())
            .find_map(|o| order_point(self, o))
            .filter(|&(_, faces)| faces)
            .and_then(|(at, _)| crate::normalised([at[0] - centre[0], at[1] - centre[1]]))
        {
            self.group_mut(id).facing = f;
        }
        for order in g.orders {
            let Some(r) = crate::commands::ordered(&order).filter(|r| g.organizations.contains(r))
            else {
                continue;
            };
            let given = self.aim_for(id, &order).unwrap_or(order);
            self.hand(g.owner, r, given);
        }
        self.mark_taking_part(id);
    }

    /// Gives `order` to the organization at `r` for its group, keeping
    /// what it has queued; whether it took it.
    fn hand(&mut self, owner: u32, r: usize, order: Command) -> bool {
        let queue = std::mem::take(&mut self.organizations[r].queue);
        let taken = self.give_order(owner, order, true).is_ok();
        self.organizations[r].queue = queue;
        taken
    }

    /// Whether `piece` is no longer on the map.
    fn piece_gone(&self, piece: Piece) -> bool {
        match piece {
            Piece::Organization(e) => self.destroyed(e),
            Piece::Building(b) => !self.buildings[b].standing(),
        }
    }

    /// Ends the group `id`'s order: puts it at `point` (its centre with
    /// none) and, unless the order was to stop or more are queued, sends
    /// each regiment to its place round there.
    fn finish_order(&mut self, id: u32, point: Option<[f32; 2]>) {
        let g = self.group(id).clone();
        let at = point.unwrap_or_else(|| self.group_centre(&g.organizations));
        let stopped = g.orders.iter().any(|o| matches!(o, Command::Stop { .. }));
        let g = self.group_mut(id);
        g.orders = Vec::new();
        g.at = Some(at);
        let (owner, members, queued) = (g.owner, g.organizations.clone(), !g.queue.is_empty());
        if stopped || queued {
            return;
        }
        self.send_to_places(id, owner, &members, at);
    }

    /// Sends each of `members` of the group `id` on a move to its place
    /// round `at`.
    fn send_to_places(&mut self, id: u32, owner: u32, members: &[usize], at: [f32; 2]) {
        for &r in members {
            if let Some(to) = self.place_round(id, r, at) {
                let organization = r;
                self.hand(owner, r, Command::Move { organization, to });
            }
        }
    }

    /// Whether the group `id`'s order is done: every regiment at rest or
    /// reforming, or the thing it's on gone; and where it's then put, if
    /// not at its centre.
    fn order_done(&self, id: u32) -> Option<Option<[f32; 2]>> {
        let g = self.group(id);
        let piece = g.orders.iter().find_map(order_piece);
        if piece.is_some_and(|p| self.piece_gone(p)) {
            return Some(None);
        }
        let done = g.organizations.iter().all(|&r| {
            self.doing_nothing(r)
                || matches!(
                    self.organizations[r].mind,
                    crate::morale::Mind::Reforming { .. }
                )
        });
        let point = match piece {
            Some(p) => self.middle(p),
            None => g
                .orders
                .iter()
                .find_map(|o| order_point(self, o))
                .map_or_else(|| self.group_centre(&g.organizations), |(at, _)| at),
        };
        done.then_some(Some(point))
    }

    /// Has the group `id`, when its regiments have just come to share a
    /// formation, under a move or guard order give that order again, and
    /// under any other or none send them to their new places round its
    /// centre; in a fight, nothing.
    fn reorder_on_sharing(&mut self, id: u32) {
        let g = self.group(id);
        let shared = self.shared_formation(&g.organizations).is_some();
        let before = g.shared;
        self.group_mut(id).shared = shared;
        if !shared || before || self.group(id).fight.is_some() {
            return;
        }
        self.work_out_places(id);
        let g = self.group(id).clone();
        let moving = g.orders.iter().any(|o| {
            matches!(
                o,
                Command::Move { .. }
                    | Command::MoveOnto { .. }
                    | Command::Guard { .. }
                    | Command::GuardPiece { .. }
            )
        });
        match moving {
            true => self.order_again(id),
            false => {
                let centre = self.group_centre(&g.organizations);
                self.send_to_places(id, g.owner, &g.organizations, centre);
            }
        }
    }

    /// The formation all of `organizations` are in, if they're in one.
    fn shared_formation(&self, organizations: &[usize]) -> Option<&String> {
        let formation = |r: usize| {
            let o = &self.organizations[r];
            (!o.broken()).then(|| o.formations.get(o.current_formation()).map(|f| &f.id))?
        };
        let first = organizations.first().and_then(|&r| formation(r))?;
        (organizations.iter())
            .all(|&r| formation(r) == Some(first))
            .then_some(first)
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
        if let Some(g) = self.groups.iter_mut().find(|g| g.id == id)
            && let Some(i) = g.organizations.iter().position(|&o| o == r)
        {
            g.organizations.remove(i);
            if i < g.places.len() {
                g.places.remove(i);
            }
        }
        self.groups.retain(|g| !g.organizations.is_empty());
    }

    /// The radius of the zone of a group of `organizations`: the widest
    /// control zone among them, and no narrower than [`ZONE_RANGE_MIN`].
    fn zone_radius(&self, organizations: &[usize]) -> f32 {
        (organizations.iter())
            .map(|&r| self.organizations[r].control_range)
            .fold(ZONE_RANGE_MIN, f32::max)
    }

    /// Marks those of the group `id`'s regiments inside its zone, round
    /// the average of their positions, as taking part in its timing, and
    /// the rest as not.
    fn mark_taking_part(&mut self, id: u32) {
        let Some(g) = self.groups.iter().find(|g| g.id == id) else {
            return;
        };
        let centre = self.group_centre(&g.organizations);
        let reach = self.zone_radius(&g.organizations);
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

    /// Each tick, for each group: takes out what's routing, exhausted,
    /// gone or no longer its owner's; sees to a fight it's in; and once a
    /// second otherwise looks for a fight, finishes an order that's done,
    /// takes up the next it has queued, and times its regiments.
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
            if !self.groups.iter().any(|g| g.id == id) {
                continue;
            }
            if let Some(due) = self.group(id).fight {
                if ticks >= due {
                    self.see_to_fight(id);
                }
                continue;
            }
            if !(ticks + u64::from(id)).is_multiple_of(BEAT) {
                continue;
            }
            self.reorder_on_sharing(id);
            if self.group_in_fight(id) {
                self.mark_taking_part(id);
                self.work_out_places(id);
                for r in self.group(id).organizations.clone() {
                    self.organizations[r].group_speed = None;
                }
                self.group_mut(id).fight = Some(ticks + 1);
                continue;
            }
            if self.group(id).orders.is_empty() {
                continue;
            }
            if let Some(point) = self.order_done(id) {
                self.finish_order(id, point);
                continue;
            }
            self.time_group(id);
        }
    }

    /// Sees to the fight the group `id` is in, once a second: each of its
    /// regiments taking part that isn't fighting itself guards the last in
    /// its order that is, or else stops. With none fighting, the fight is
    /// over: those taking part stop, and the group gives its order again,
    /// or with none, is put at its centre.
    fn see_to_fight(&mut self, id: u32) {
        let g = self.group(id).clone();
        let fighting =
            |w: &World, r: usize| !w.organizations[r].engaged.is_empty() || w.under_siege(r);
        let last = (g.organizations.iter().copied()).rfind(|&r| fighting(self, r));
        let idle: Vec<usize> = (g.organizations.iter().copied())
            .filter(|&r| self.organizations[r].taking_part && !fighting(self, r))
            .collect();
        let Some(last) = last else {
            for &organization in &idle {
                self.hand(g.owner, organization, Command::Stop { organization });
            }
            self.group_mut(id).fight = None;
            match g.orders.is_empty() {
                true => self.finish_order(id, None),
                false => self.order_again(id),
            }
            return;
        };
        for organization in idle {
            let piece = Piece::Organization(last);
            let guard = Command::GuardPiece {
                organization,
                piece,
            };
            if !self.hand(g.owner, organization, guard) {
                self.hand(g.owner, organization, Command::Stop { organization });
            }
        }
        self.group_mut(id).fight = Some(self.ticks + BEAT);
    }

    /// Whether the group `id` is in a fight, as the first of its
    /// regiments taking part that is engaged or under siege has it: under
    /// siege, or engaged other than with what it was sent to attack.
    fn group_in_fight(&self, id: u32) -> bool {
        let g = self.groups.iter().find(|g| g.id == id).expect("listed");
        let first = (g.organizations.iter().copied()).find(|&r| {
            let o = &self.organizations[r];
            o.taking_part && (!o.engaged.is_empty() || self.under_siege(r))
        });
        first.is_some_and(|r| {
            let engaged = &self.organizations[r].engaged;
            self.under_siege(r) || !self.attack_target(r).is_some_and(|t| engaged.contains(&t))
        })
    }

    /// Whether the organization at `r` is under siege, as only a lone
    /// unit can be.
    pub(crate) fn under_siege(&self, r: usize) -> bool {
        self.lone(r) && self.survivors(r).any(|u| self.units[u].besieged.is_some())
    }

    /// What the organization at `r` was sent to attack: the enemy it's
    /// pursuing or was moved onto, or failing those while it's engaged,
    /// the first of its queued orders on an enemy.
    fn attack_target(&self, r: usize) -> Option<Piece> {
        let o = &self.organizations[r];
        let enemy = |p: Piece| self.hostile(r, p).then_some(p);
        let ordered = match o.order {
            crate::Order::Pursue(t) => Some(Piece::Organization(t)),
            _ => o.follow.and_then(enemy),
        };
        let queued = || {
            (!o.engaged.is_empty())
                .then(|| {
                    o.queue
                        .iter()
                        .find_map(|c| queued_target(c).and_then(enemy))
                })
                .flatten()
        };
        ordered.or_else(queued)
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
            enemy.then_some(range)
        });
        let times: Vec<(usize, f32, f32)> = (g.organizations.iter().enumerate())
            .filter(|&(_, &r)| {
                (g.times_all || self.organizations[r].taking_part) && self.underway(r)
            })
            .filter_map(|(i, &r)| {
                let speed = self.unhurried_speed(r);
                let lp = self.pacer(r)?;
                let mut left = self.units[self.organizations[r].units[lp]].route_left();
                if let (Some(range), Some(&[_, ahead])) = (zone, g.places.get(i)) {
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
    /// the positions its group formation gives them in turn; otherwise,
    /// or past its last position, where each stands, one not taking part
    /// put at the zone's least radius in its own direction. Either way set
    /// about the centre.
    fn group_places(&self, organizations: &[usize], facing: [f32; 2]) -> Vec<[f32; 2]> {
        let given = self.shared_formation(organizations).and_then(|id| {
            self.rules
                .group_formations
                .get(&tgmap::Id::from(id.as_str()))
        });
        let centre = self.group_centre(organizations);
        let standing = |r: usize| {
            let at = tgmap::to_local(centre, facing, self.centre(r));
            match self.organizations[r].taking_part {
                true => at,
                false => crate::normalised(at)
                    .map_or(at, |[x, y]| [x * ZONE_RANGE_MIN, y * ZONE_RANGE_MIN]),
            }
        };
        let places: Vec<[f32; 2]> = (organizations.iter().enumerate())
            .map(|(i, &r)| match given.and_then(|p| p.get(i)) {
                Some(&at) => at,
                None => standing(r),
            })
            .collect();
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

/// What a queued `command` would attack, if it's an order on a thing.
fn queued_target(command: &Command) -> Option<Piece> {
    match *command {
        Command::Queued { ref command } => queued_target(command),
        Command::Pursue { enemy, .. } => Some(Piece::Organization(enemy)),
        Command::MoveOnto { piece, .. } => Some(piece),
        _ => None,
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
    fn a_detach_order_goes_to_the_group_as_one_or_takes_one_out() {
        let mut w = grouped();
        for r in [0, 1] {
            w.organizations[r].hq = Some(99);
        }
        let detach = |organization| Command::Detach { organization };
        w.apply(
            7,
            Command::Group {
                orders: vec![detach(0), detach(1)],
            },
        )
        .unwrap();
        let group = w.organizations[0].group;
        assert!(group.is_some() && w.organizations[1].group == group);
        // Given to one alone, it takes that one out.
        w.organizations[1].hq = Some(99);
        w.apply(7, detach(1)).unwrap();
        assert_eq!(w.organizations[1].group, None);
    }

    /// `grouped`, with player 8's two regiments beside it.
    fn grouped_with_enemies() -> crate::World {
        let mut w = grouped();
        let side = |id, relations: &[u32]| crate::Player {
            independent: false,
            ..player(id, None, relations, &[])
        };
        w.sides.players = vec![side(7, &[0, 3]), side(8, &[3, 0])];
        for _ in 0..2 {
            let mut enemy = w.organizations[0].clone();
            enemy.owner = 8;
            enemy.group = None;
            w.organizations.push(enemy);
        }
        w
    }

    #[test]
    fn a_group_fighting_what_it_was_sent_to_attack_isnt_in_a_fight() {
        use crate::Piece::Organization;
        let mut w = grouped_with_enemies();
        assert!(w.hostile(0, Organization(2)));
        w.apply(7, moves([[9.0, 50.0], [11.0, 30.0]])).unwrap();
        let id = w.organizations[0].group.unwrap();
        assert!(!w.group_in_fight(id));
        w.organizations[0].order = crate::Order::Pursue(2);
        w.organizations[0].engaged = vec![Organization(2)];
        assert!(!w.group_in_fight(id));
        // Drawn into another fight, it is.
        w.organizations[0].engaged = vec![Organization(3)];
        assert!(w.group_in_fight(id));
        // Engaged, a queued order on an enemy is what it's after.
        w.organizations[0].order = crate::Order::Move;
        let queued = Command::Pursue {
            organization: 0,
            enemy: 3,
        };
        w.organizations[0].queue = vec![queued];
        assert!(!w.group_in_fight(id));
        // Only the first engaged is looked at.
        w.organizations[1].engaged = vec![Organization(2)];
        assert!(!w.group_in_fight(id));
    }

    #[test]
    fn a_group_with_a_lone_unit_under_siege_is_in_a_fight() {
        let mut w = grouped();
        w.apply(7, moves([[9.0, 50.0], [11.0, 30.0]])).unwrap();
        let id = w.organizations[0].group.unwrap();
        w.units[1].besieged = Some(w.ticks + 100);
        // A regiment as a whole never is.
        assert!(!w.group_in_fight(id));
        w.units[1].template = w.organizations[1].template.clone();
        assert!(w.group_in_fight(id));
    }

    #[test]
    fn a_group_gives_its_order_again_once_its_regiments_share_a_new_formation() {
        let mut w = grouped();
        w.apply(7, moves([[9.0, 50.0], [11.0, 50.0]])).unwrap();
        run(&mut w, 1.0);
        let formation = |organization| Command::Formation {
            organization,
            formation: 1,
        };
        w.apply(7, formation(1)).unwrap();
        run(&mut w, 1.0);
        // The second stops short, as one whose move failed would.
        let stopped = w.units[1].position;
        w.units[1].goal = None;
        w.units[1].path.clear();
        w.organizations[1].position = stopped;
        run(&mut w, 1.0);
        assert!(!w.underway(1));
        w.apply(7, formation(0)).unwrap();
        run(&mut w, 1.0);
        assert!(w.underway(1));
        assert_eq!(w.organizations[1].group, w.organizations[0].group);
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
    fn a_regiment_waiting_for_its_group_neither_moves_nor_turns() {
        let mut w = grouped();
        w.units[1].turn_radius = 3.0;
        let order = moves([[9.0, 55.0], [11.0, 12.0]]);
        w.apply(7, order).unwrap();
        while !w.waiting_for_group(1) {
            run(&mut w, 0.1);
        }
        // Facing away from where it's going.
        w.units[1].facing = [1.0, 0.0];
        let at = w.units[1].position;
        run(&mut w, 0.9);
        assert!(w.waiting_for_group(1));
        assert_eq!((w.units[1].position, w.units[1].facing), (at, [1.0, 0.0]));
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

    #[test]
    fn regiments_past_the_group_formations_last_position_keep_where_they_stand() {
        let mut w = grouped();
        w.rules
            .group_formations
            .insert("line".into(), vec![[0.0, 6.0]]);
        for r in [0, 1] {
            w.organizations[r].taking_part = true;
        }
        // The second, 1 right of the centre, then set about it with the
        // first.
        let places = w.group_places(&[0, 1], [0.0, 1.0]);
        assert!(
            near(places[0], [-0.5, 3.0]) && near(places[1], [0.5, -3.0]),
            "{places:?}"
        );
    }

    /// `grouped`, whose line's group formation sets the two 12 apart.
    fn in_line() -> crate::World {
        let mut w = grouped();
        let line = vec![[-6.0, 0.0], [6.0, 0.0]];
        w.rules.group_formations.insert("line".into(), line);
        w
    }

    #[test]
    fn a_move_faces_the_group_its_way_and_sends_each_for_its_place_round_the_point() {
        let mut w = in_line();
        w.apply(7, moves([[10.0, 50.0], [10.0, 50.0]])).unwrap();
        let id = w.organizations[0].group.unwrap();
        assert!(near(w.group(id).facing, [0.0, 1.0]));
        assert!(
            near(w.destination(0), [4.0, 50.0]),
            "{:?}",
            w.destination(0)
        );
        assert!(near(w.destination(1), [16.0, 50.0]));
        // An attack leaves it facing as it was.
        let attack = |organization| Command::Attack {
            organization,
            to: [50.0, 10.0],
        };
        let orders = vec![attack(0), attack(1)];
        w.apply(7, Command::Group { orders }).unwrap();
        let id = w.organizations[0].group.unwrap();
        assert!(near(w.group(id).facing, [0.0, -1.0]), "a fresh group's");
    }

    #[test]
    fn a_place_outside_the_groups_zone_is_pulled_in_toward_the_point() {
        let mut w = grouped();
        let wide = vec![[-40.0, 0.0], [40.0, 0.0]];
        w.rules.group_formations.insert("line".into(), wide);
        w.apply(7, moves([[30.0, 50.0], [30.0, 50.0]])).unwrap();
        for r in [0, 1] {
            let to = w.destination(r);
            assert!(crate::distance(to, [30.0, 50.0]) <= 18.0, "{to:?}");
            assert_eq!(tgmap::Tile::at(to).centre(), to);
        }
    }

    #[test]
    fn a_group_done_with_its_order_is_put_at_its_point_and_takes_up_the_next_from_there() {
        let mut w = in_line();
        w.apply(7, moves([[10.0, 30.0], [10.0, 30.0]])).unwrap();
        let id = w.organizations[0].group.unwrap();
        let queued = crate::Command::Queued {
            command: Box::new(moves([[40.0, 30.0], [40.0, 30.0]])),
        };
        w.apply(7, queued).unwrap();
        assert!((0..60 * SECOND).any(|_| {
            w.tick();
            w.group(id).at.is_some()
        }));
        assert_eq!(w.group(id).at, Some([10.0, 30.0]));
        // From the last point, east.
        run(&mut w, 2.0);
        assert!(
            near(w.group(id).facing, [1.0, 0.0]),
            "{:?}",
            w.group(id).facing
        );
        assert_eq!(w.organizations[1].group, Some(id));
    }

    #[test]
    fn in_a_fight_the_rest_guard_the_last_engaged_then_take_up_the_order_again() {
        use crate::Piece::Organization;
        let mut w = grouped_with_enemies();
        w.apply(7, moves([[9.0, 50.0], [11.0, 50.0]])).unwrap();
        let id = w.organizations[0].group.unwrap();
        w.organizations[0].engaged = vec![Organization(2)];
        w.see_to_fight(id);
        let guarding = w.organizations[1].guard.and_then(|g| g.of);
        assert_eq!(guarding, Some(Organization(0)));
        assert_eq!(w.organizations[1].group, Some(id));
        // Fighting what it was sent to attack still counts here.
        w.organizations[0].order = crate::Order::Pursue(2);
        w.see_to_fight(id);
        assert!(w.organizations[1].guard.is_some());
        // With none engaged, the fight is over and the move given again.
        w.organizations[0].engaged.clear();
        w.organizations[0].order = crate::Order::Move;
        w.group_mut(id).fight = Some(w.ticks);
        w.see_to_fight(id);
        assert_eq!(w.group(id).fight, None);
        assert_eq!(w.organizations[1].guard, None);
        assert!(w.underway(1));
    }

    #[test]
    fn a_computer_players_group_times_every_moving_regiment() {
        let mut w = grouped();
        w.sides.computers.insert(7, "computer".into());
        w.apply(7, moves([[9.0, 50.0], [11.0, 30.0]])).unwrap();
        let id = w.organizations[0].group.unwrap();
        assert!(w.group(id).times_all);
        w.organizations[1].taking_part = false;
        w.time_group(id);
        assert!(w.organizations[1].group_speed.is_some());
    }
}
