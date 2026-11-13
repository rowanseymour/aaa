//! HQs and the regiments attached to them, which is what lets a regiment
//! be in supply.

use aaa_map::{Id, Logistics};

use crate::{Refused, World, distance};

/// A Division HQ as it stands: packed, a regiment of its truck, or
/// unpacked, a building.
#[derive(Debug, Clone, PartialEq)]
pub struct Hq {
    /// Its organization's object id, the same packed and unpacked.
    pub id: u32,
    pub owner: u32,
    /// The template of the building it is, or unpacks into.
    pub building: String,
    pub position: [f32; 2],
    /// How many regiments it takes.
    pub places: u32,
}

impl World {
    pub(crate) fn logistics_of(&self, template: &str) -> Option<Logistics> {
        self.logistics.get(&Id::from(template)).copied()
    }

    fn places(&self, template: &str) -> Option<u32> {
        self.logistics_of(template)?.attachments
    }

    /// The HQs on the map, in the order the attach order steps through.
    pub fn hqs(&self) -> Vec<Hq> {
        let mut out = Vec::new();
        // Packed, each is its regiment for as long as its truck lasts.
        for (r, regiment) in self.regiments.iter().enumerate() {
            let places = self.places(&regiment.template);
            let (Some(places), Some(unpacks)) = (places, &regiment.unpacks) else {
                continue;
            };
            let building = &unpacks.building.template;
            if self.truck_survives(r) {
                out.push(Hq {
                    id: regiment.id,
                    owner: regiment.owner,
                    building: building.clone(),
                    position: self.centre(r),
                    places,
                });
            }
        }
        for (b, building) in self.buildings.iter().enumerate() {
            if let Some((id, places, _)) = self.hq_behind(b).filter(|_| !building.gone()) {
                out.push(Hq {
                    id,
                    owner: building.owner,
                    building: building.template.clone(),
                    position: building.position,
                    places,
                });
            }
        }
        out.sort_by_key(|h| h.id);
        out
    }

    /// Whether any of the truck a packed building's regiment is still
    /// lives: the units its building packs into, or any where the data
    /// doesn't say.
    fn truck_survives(&self, r: usize) -> bool {
        let unpacks = self.regiments[r].unpacks.as_ref();
        let trucks = unpacks
            .and_then(|u| self.packing.get(&u.building.template))
            .map(|p| &p.truck.members);
        let truck = |u: usize| {
            trucks.is_none_or(|t| t.iter().any(|m| m.template == self.units[u].template))
        };
        self.survivors(r).any(truck)
    }

    /// The id, places and organization template of the HQ the building at
    /// `b` is, if it's one: the organization a map has it standing in, or
    /// the regiment it unpacked from.
    fn hq_behind(&self, b: usize) -> Option<(u32, u32, &str)> {
        let building = &self.buildings[b];
        let (id, template) = match (&building.organization, building.unpacked_from) {
            (Some(o), _) => (o.id, o.template.as_str()),
            (None, Some(r)) => (self.regiments[r].id, self.regiments[r].template.as_str()),
            (None, None) => return None,
        };
        Some((id, self.places(template)?, template))
    }

    /// The template of the organization the building at `b` is the HQ of,
    /// if it's one.
    pub fn hq_template(&self, b: usize) -> Option<&str> {
        self.hq_behind(b).map(|(.., template)| template)
    }

    /// The HQ the regiment at `r` is while it's packed, if it's one.
    pub fn packed_hq(&self, r: usize) -> Option<u32> {
        let regiment = &self.regiments[r];
        self.places(&regiment.template)?;
        (regiment.unpacks.is_some() && self.truck_survives(r)).then_some(regiment.id)
    }

    /// `player`'s regiments that have no HQ, by index, whether or not any
    /// HQ takes their kind. An HQ isn't one, nor is a unit outside an
    /// organization, such as a depot's truck.
    pub fn detached(&self, player: u32) -> Vec<usize> {
        let regimental = |r: usize| {
            let logistics = self.logistics_of(&self.regiments[r].template);
            logistics.is_some_and(|l| l.attachments.is_none())
        };
        (0..self.regiments.len())
            .filter(|&r| {
                let regiment = &self.regiments[r];
                regiment.owner == player
                    && regiment.hq.is_none()
                    && regimental(r)
                    && !self.destroyed(r)
            })
            .collect()
    }

    /// Whether any of `player`'s detached regiments is of a kind one of its
    /// HQs takes, whether or not the HQ has a place.
    pub fn attachable(&self, player: u32) -> bool {
        let hqs = self.hqs();
        let detached = self.detached(player);
        (detached.iter()).any(|&r| hqs.iter().any(|h| self.suits(h, r)))
    }

    /// The HQ the building at `b` is, if it's one.
    pub fn hq_of(&self, b: usize) -> Option<u32> {
        self.hq_behind(b).map(|(id, ..)| id)
    }

    /// The regiments attached to the HQ `hq`, by index.
    pub fn attached(&self, hq: u32) -> impl Iterator<Item = usize> + '_ {
        (0..self.regiments.len())
            .filter(move |&r| self.regiments[r].hq == Some(hq) && !self.destroyed(r))
    }

    /// How many more regiments an HQ can take: its places less those
    /// attached and those its building has yet to put out.
    pub fn free_places(&self, hq: &Hq) -> u32 {
        let queued: usize = (0..self.buildings.len())
            .filter(|&b| self.hq_of(b) == Some(hq.id))
            .map(|b| self.buildings[b].queue.len())
            .sum();
        let taken = self.attached(hq.id).count() + queued;
        hq.places.saturating_sub(taken as u32)
    }

    /// Whether an HQ is one the regiment at `r` could attach to, places
    /// aside: its own side's, and one whose building recruits its kind.
    fn suits(&self, hq: &Hq, r: usize) -> bool {
        let regiment = &self.regiments[r];
        let recruits = self.recruit_lists.get(&Id::from(hq.building.as_str()));
        hq.owner == regiment.owner
            && recruits.is_some_and(|l| l.contains(&Id::from(regiment.template.as_str())))
    }

    /// The HQ an order to attach would put the regiment at `r` in: the
    /// nearest with a place for one that's detached, else the next after
    /// its own, round to the first again.
    pub fn attaches_to(&self, r: usize) -> Option<u32> {
        let hqs = self.hqs();
        let open = |h: &&Hq| self.suits(h, r) && self.free_places(h) > 0;
        let at = self.leader_position(r);
        match self.regiments[r]
            .hq
            .and_then(|id| hqs.iter().position(|h| h.id == id))
        {
            Some(own) => {
                let after = hqs.iter().cycle().skip(own + 1).take(hqs.len() - 1);
                after.filter(open).map(|h| h.id).next()
            }
            None => (hqs.iter().filter(open))
                .min_by(|a, b| distance(at, a.position).total_cmp(&distance(at, b.position)))
                .map(|h| h.id),
        }
    }

    pub(crate) fn may_attach(&self, player: u32, r: usize) -> Result<u32, Refused> {
        self.may_command(player, r)?;
        let used = self.logistics_of(&self.regiments[r].template);
        if !used.is_some_and(|l| l.supply_used) {
            return Err(Refused::Unable);
        }
        self.attaches_to(r).ok_or(Refused::Unable)
    }

    pub(crate) fn may_detach(&self, player: u32, r: usize) -> Result<(), Refused> {
        self.may_command(player, r)?;
        match self.regiments[r].hq {
            Some(_) => Ok(()),
            None => Err(Refused::Unable),
        }
    }

    /// Detaches the regiment at `r` if its HQ isn't among `hqs` any more.
    /// It keeps one of another side, as it does on changing side itself.
    pub(crate) fn keep_attached(&mut self, r: usize, hqs: &[Hq]) {
        let regiment = &mut self.regiments[r];
        regiment.hq = (regiment.hq).filter(|&id| hqs.iter().any(|h| h.id == id));
    }

    /// Detaches every regiment attached to the HQ `hq`.
    pub(crate) fn detach_all(&mut self, hq: u32) {
        for regiment in &mut self.regiments {
            regiment.hq = regiment.hq.filter(|&id| id != hq);
        }
    }

    /// Hands the regiment at `r` to `owner`. It keeps its place at its
    /// HQ, though that's its old side's; a packed HQ loses its regiments.
    pub fn change_side(&mut self, r: usize, owner: u32) {
        if let Some(hq) = self.packed_hq(r) {
            self.detach_all(hq);
        }
        self.regiments[r].owner = owner;
    }
}

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use aaa_map::Id;

    use crate::{Command, Notice, Refused};

    #[test]
    fn regiments_attach_to_hqs_that_recruit_their_kind_and_have_room() {
        let mut w = division();
        assert_eq!(w.hqs().len(), 1);
        assert_eq!(w.hqs()[0].id, 50);
        // Packed, it's its regiment, and the company is the one detached.
        assert_eq!((w.packed_hq(0), w.packed_hq(1)), (Some(50), None));
        assert_eq!((w.detached(1), w.detached(2)), (vec![1], vec![]));
        assert!(w.attachable(1) && !w.attachable(2));
        // The HQ itself is in no supply and attaches to nothing.
        assert_eq!(
            w.check(1, &Command::Attach { regiment: 0 }),
            Err(Refused::Unable)
        );
        assert_eq!(
            w.check(1, &Command::Detach { regiment: 1 }),
            Err(Refused::Unable)
        );
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        assert_eq!(w.regiments[1].hq, Some(50));
        assert_eq!(w.attached(50).collect::<Vec<_>>(), [1]);
        // No other HQ to step on to.
        assert_eq!(
            w.check(1, &Command::Attach { regiment: 1 }),
            Err(Refused::Unable)
        );
        // A second one, nearer: a detached regiment takes the nearest, an
        // attached one the next round.
        let mut t = truck();
        (t.id, t.owner) = (70, 1);
        t.members[0].id = 70;
        t.place([26.0, 24.0], [0.0, 1.0]);
        w.add_regiment(&t);
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        assert_eq!(w.regiments[1].hq, Some(70));
        assert!(w.detached(1).is_empty());
        w.notices.clear();
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        assert_eq!(w.regiments[1].hq, Some(50));
        // The player's told of each, a move as leaving then joining.
        let (player, regiment) = (1, 1);
        let left = |hq| Notice::Detached {
            player,
            hq,
            regiment,
        };
        let joined = |hq| Notice::Attached {
            player,
            hq,
            regiment,
        };
        assert_eq!(w.notices, [left(70), joined(50)]);
        w.apply(1, Command::Detach { regiment: 1 }).unwrap();
        assert_eq!(w.notices[2..], [left(50)]);
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        assert_eq!(w.regiments[1].hq, Some(70));
        // Not an enemy's, nor one that doesn't recruit its kind.
        w.regiments[1].hq = None;
        w.regiments[0].owner = 2;
        w.recruit_lists.clear();
        assert_eq!(w.attaches_to(1), None);
    }

    #[test]
    fn regiments_no_hq_recruits_are_detached_too() {
        let mut w = division();
        // Agents, which nothing recruits, and a truck outside any
        // organization.
        w.logistics.insert(Id::from("agents"), logistics(None));
        let mut agents = raised_company(&mut w, 1);
        agents.renumber(80);
        agents.template = "agents".into();
        agents.place([20.0, 20.0], [0.0, 1.0]);
        w.add_regiment(&agents);
        let mut lorry = truck();
        (lorry.id, lorry.owner, lorry.template) = (90, 1, "lorry".into());
        lorry.members[0].id = 90;
        lorry.place([22.0, 30.0], [0.0, 1.0]);
        w.add_regiment(&lorry);
        assert_eq!(w.detached(1), [1, 2]);
        // Only the company is one an HQ could take.
        assert!(w.attachable(1));
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        assert_eq!(w.detached(1), [2]);
        assert!(!w.attachable(1));
    }

    #[test]
    fn regiments_lose_their_hq_when_its_destroyed() {
        let mut w = division();
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        run(&mut w, 1.0);
        assert!(w.regiments[1].in_supply);
        w.destroy(0);
        run(&mut w, 1.0);
        assert_eq!(w.regiments[1].hq, None);
        assert!(!w.regiments[1].in_supply);
    }

    #[test]
    fn regiments_that_change_side_keep_their_place_at_their_hq() {
        let mut w = supplied_division();
        let hq = w.hqs().remove(0);
        // With an ally, its old side's zones are still its own.
        w.change_side(1, 3);
        run(&mut w, 1.0);
        assert_eq!(w.regiments[1].hq, Some(50));
        assert!(w.regiments[1].in_supply);
        assert_eq!(w.free_places(&hq), 1);
        // With an enemy it's still attached, but needs that side's zones.
        w.change_side(1, 2);
        run(&mut w, 1.0);
        assert_eq!(w.regiments[1].hq, Some(50));
        assert!(!w.regiments[1].in_supply);
        assert_eq!(w.free_places(&hq), 1);
        // An HQ that changes side loses its regiments at once.
        w.change_side(0, 3);
        assert_eq!(w.regiments[1].hq, None);
    }

    #[test]
    fn captured_hqs_lose_their_regiments_at_once() {
        let mut w = division();
        let hq = Command::Unpack {
            regiment: 0,
            at: [30.0, 40.0],
            facing: NORTH,
        };
        w.apply(1, hq).unwrap();
        run(&mut w, 30.0);
        let b = w.regiments[0].unpacked_into.expect("unpacked");
        w.apply(1, Command::Attach { regiment: 1 }).unwrap();
        w.change_hands(b, 2);
        assert_eq!(w.regiments[1].hq, None);
    }
}
