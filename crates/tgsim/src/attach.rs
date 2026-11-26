//! HQs and the organizations attached to them, which is what lets an
//! organization be in supply.

use tgmap::{Id, Logistics};

use crate::{Refused, World, distance};

/// A Division HQ as it stands: packed, an organization of its truck, or
/// unpacked, a building.
#[derive(Debug, Clone, PartialEq)]
pub struct Hq {
    /// Its organization's object id, the same packed and unpacked.
    pub id: u32,
    pub owner: u32,
    /// The template of the building it is, or unpacks into.
    pub building: Id,
    pub position: [f32; 2],
    /// How many organizations it takes.
    pub places: u32,
}

impl World {
    pub(crate) fn logistics_of(&self, template: &Id) -> Option<Logistics> {
        self.rules.logistics.get(template).copied()
    }

    fn places(&self, template: &Id) -> Option<u32> {
        self.logistics_of(template)?.attachments
    }

    /// The HQs on the map, in the order the attach order steps through.
    pub fn hqs(&self) -> Vec<Hq> {
        let mut out = Vec::new();
        // Packed, each is its organization for as long as its truck lasts.
        for (r, organization) in self.organizations.iter().enumerate() {
            let places = self.places(&organization.template);
            let (Some(places), Some(unpacks)) = (places, &organization.unpacks) else {
                continue;
            };
            let building = &unpacks.building.template;
            if self.truck_survives(r) {
                out.push(Hq {
                    id: organization.id,
                    owner: organization.owner,
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

    /// Whether any of the truck a packed building's organization is still
    /// lives: the units its building packs into, or any where the data
    /// doesn't say.
    fn truck_survives(&self, r: usize) -> bool {
        let unpacks = self.organizations[r].unpacks.as_ref();
        let trucks = unpacks
            .and_then(|u| self.rules.packing.get(&u.building.template))
            .map(|p| &p.truck.members);
        let truck = |u: usize| {
            trucks.is_none_or(|t| t.iter().any(|m| m.template == self.units[u].template))
        };
        self.survivors(r).any(truck)
    }

    /// The id, places and organization template of the HQ the building at
    /// `b` is, if it's one: the organization a map has it standing in, or
    /// the organization it unpacked from.
    fn hq_behind(&self, b: usize) -> Option<(u32, u32, &str)> {
        let building = &self.buildings[b];
        match (&building.organization, building.unpacked_from) {
            (Some(o), _) => {
                let places = self.places(&o.template)?;
                Some((o.id, places, o.template.as_str()))
            }
            (None, Some(r)) => {
                let organization = &self.organizations[r];
                let places = self.places(&organization.template)?;
                Some((organization.id, places, organization.template.as_str()))
            }
            (None, None) => None,
        }
    }

    /// The template of the organization the building at `b` is the HQ of,
    /// if it's one.
    pub fn hq_template(&self, b: usize) -> Option<&str> {
        self.hq_behind(b).map(|(.., template)| template)
    }

    /// The HQ the organization at `r` is while it's packed, if it's one.
    pub fn packed_hq(&self, r: usize) -> Option<u32> {
        self.packed_as(r, true)
    }

    /// The HQ the organization at `r` was while packed, if its truck is gone.
    pub(crate) fn truckless_hq(&self, r: usize) -> Option<u32> {
        self.packed_as(r, false)
    }

    /// The HQ id of a packed HQ organization at `r` whose truck's survival is
    /// `survives`.
    fn packed_as(&self, r: usize, survives: bool) -> Option<u32> {
        let organization = &self.organizations[r];
        self.places(&organization.template)?;
        let packed = organization.unpacks.is_some() && self.truck_survives(r) == survives;
        packed.then_some(organization.id)
    }

    /// `player`'s organizations that have no HQ, by index, whether or not any
    /// HQ takes their kind. An HQ isn't one, nor is a unit outside an
    /// organization, such as a depot's truck.
    pub fn detached(&self, player: u32) -> Vec<usize> {
        let organizational = |r: usize| {
            let logistics = self.logistics_of(&self.organizations[r].template);
            logistics.is_some_and(|l| l.attachments.is_none())
        };
        (0..self.organizations.len())
            .filter(|&r| {
                let organization = &self.organizations[r];
                organization.owner == player
                    && organization.hq.is_none()
                    && organizational(r)
                    && !self.destroyed(r)
            })
            .collect()
    }

    /// Whether any of `player`'s detached organizations is of a kind one of its
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

    /// The organizations attached to the HQ `hq`, by index.
    pub fn attached(&self, hq: u32) -> impl Iterator<Item = usize> + '_ {
        (0..self.organizations.len())
            .filter(move |&r| self.organizations[r].hq == Some(hq) && !self.destroyed(r))
    }

    /// How many more organizations an HQ can take: its places less those
    /// attached, those its building has yet to put out and those on
    /// their way by air.
    pub fn free_places(&self, hq: &Hq) -> u32 {
        let queued: usize = (0..self.buildings.len())
            .filter(|&b| self.hq_of(b) == Some(hq.id))
            .map(|b| self.buildings[b].queue.len())
            .sum();
        let taken = self.attached(hq.id).count() + queued + self.bound_for(hq.id).count();
        hq.places.saturating_sub(taken as u32)
    }

    /// Whether an HQ is one the organization at `r` could attach to, places
    /// aside: its own side's, and one whose building recruits its kind.
    pub fn suits(&self, hq: &Hq, r: usize) -> bool {
        let organization = &self.organizations[r];
        let recruits = self.rules.recruit_lists.get(&hq.building);
        hq.owner == organization.owner
            && recruits.is_some_and(|l| l.contains(&organization.template))
    }

    /// The HQ an order to attach would put the organization at `r` in: the
    /// nearest with a place for one that's detached, else the next after
    /// its own, round to the first again.
    pub fn attaches_to(&self, r: usize) -> Option<u32> {
        let hqs = self.hqs();
        let open = |h: &&Hq| self.suits(h, r) && self.free_places(h) > 0;
        let at = self.leader_position(r);
        match self.organizations[r]
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
        let used = self.logistics_of(&self.organizations[r].template);
        if !used.is_some_and(|l| l.supply_used) {
            return Err(Refused::Unable);
        }
        let hq = self.attaches_to(r).ok_or(Refused::Unable)?;
        if self.attach_cost(r) > self.stockpile(player) {
            return Err(Refused::Unaffordable);
        }
        Ok(hq)
    }

    pub(crate) fn may_detach(&self, player: u32, r: usize) -> Result<(), Refused> {
        self.may_command(player, r)?;
        match self.organizations[r].hq {
            Some(_) => Ok(()),
            None => Err(Refused::Unable),
        }
    }

    /// Detaches each organization whose HQ isn't on the map, as a map's
    /// attachments are only those its HQs list.
    pub(crate) fn detach_from_missing(&mut self) {
        let ids: Vec<u32> = self.hqs().iter().map(|h| h.id).collect();
        for organization in &mut self.organizations {
            organization.hq = organization.hq.filter(|id| ids.contains(id));
        }
    }

    /// Detaches every organization attached to the HQ `hq`, and frees the
    /// places aircraft hold there, so what they bring arrives unattached.
    pub(crate) fn detach_all(&mut self, hq: u32) {
        for organization in &mut self.organizations {
            organization.hq = organization.hq.filter(|&id| id != hq);
        }
        for delivery in &mut self.deliveries {
            delivery.hq = delivery.hq.filter(|&id| id != hq);
        }
    }

    /// Hands the organization at `r` to `owner`. It keeps its place at its
    /// HQ, though that's its old side's; a packed HQ loses its organizations.
    pub fn change_side(&mut self, r: usize, owner: u32) {
        if let Some(hq) = self.packed_hq(r) {
            self.detach_all(hq);
        }
        self.organizations[r].owner = owner;
        self.refigure_morale(r, false);
    }
}

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use tgmap::Id;

    use crate::{Command, Notice, Refused};

    #[test]
    fn attaching_costs_a_share_of_the_recruit_cost_and_is_refused_unpaid() {
        let mut w = division();
        let company = raised_company(&mut w, 1);
        recruits(&mut w, "depot", company, 1.0);
        let economy = tgmap::Economy {
            cost: amounts(&[("money", 100.0)]),
            ..Default::default()
        };
        w.rules.economies.insert(Id::from("company"), economy);
        w.rules.supply_rules.attach_cost = 0.5;
        assert_eq!(w.attach_cost(1), 50.0);
        w.set_stockpile(1, 60.0);
        w.apply(1, Command::Attach { organization: 1 }).unwrap();
        assert_eq!(w.stockpile(1), 10.0);
        // Short of it, nothing's taken and the regiment stays as it was.
        w.apply(1, Command::Detach { organization: 1 }).unwrap();
        let attach = w.apply(1, Command::Attach { organization: 1 });
        assert_eq!(attach, Err(Refused::Unaffordable));
        assert_eq!((w.stockpile(1), w.organizations[1].hq), (10.0, None));
    }

    #[test]
    fn hqs_destroyed_packed_or_unpacked_detach_their_organizations_at_once() {
        let mut w = division();
        w.apply(1, Command::Attach { organization: 1 }).unwrap();
        w.destroy(0);
        assert_eq!(w.organizations[1].hq, None);

        let mut w = division();
        w.apply(1, Command::Attach { organization: 1 }).unwrap();
        let unpack = Command::Unpack {
            organization: 0,
            at: [30.0, 40.0],
            facing: NORTH,
        };
        w.apply(1, unpack).unwrap();
        run(&mut w, 11.0);
        let depot = w.organizations[0].unpacked_into.unwrap();
        assert_eq!(w.organizations[1].hq, Some(50));
        w.buildings[depot].fighter.destructible = true;
        w.fall(depot, None);
        assert_eq!(w.organizations[1].hq, None);
    }

    #[test]
    fn a_packed_hq_is_lost_with_its_truck_whoever_else_survives() {
        let mut w = division();
        w.rules.packing.insert(Id::from("depot"), packs());
        let mut t = truck();
        let mut escort = t.members[0].clone();
        escort.template = "soldier".into();
        t.members.push(escort);
        t.formations[0].offsets.push([2.0, 0.0]);
        t.owner = 1;
        t.renumber(70);
        t.place([40.0, 30.0], [0.0, 1.0]);
        let hq = w.add_organization(&t);
        w.organizations[1].hq = Some(70);
        let truck = w.organizations[hq].units[0];
        w.units[truck].die(0);
        w.lost_member(truck);
        assert!(!w.destroyed(hq));
        assert_eq!(w.organizations[1].hq, None);
    }

    #[test]
    fn attachments_to_hqs_not_on_the_map_are_dropped() {
        let mut w = division();
        w.organizations[1].hq = Some(999);
        w.detach_from_missing();
        assert_eq!(w.organizations[1].hq, None);
        w.organizations[1].hq = Some(50);
        w.detach_from_missing();
        assert_eq!(w.organizations[1].hq, Some(50));
    }

    #[test]
    fn organizations_attach_to_hqs_that_recruit_their_kind_and_have_room() {
        let mut w = division();
        assert_eq!(w.hqs().len(), 1);
        assert_eq!(w.hqs()[0].id, 50);
        // Packed, it's its organization, and the company is the one detached.
        assert_eq!((w.packed_hq(0), w.packed_hq(1)), (Some(50), None));
        assert_eq!((w.detached(1), w.detached(2)), (vec![1], vec![]));
        assert!(w.attachable(1) && !w.attachable(2));
        // The HQ itself is of a kind no HQ takes.
        let hq = &w.hqs()[0];
        assert!(w.suits(hq, 1) && !w.suits(hq, 0));
        // The HQ itself is in no supply and attaches to nothing.
        assert_eq!(
            w.check(1, &Command::Attach { organization: 0 }),
            Err(Refused::Unable)
        );
        assert_eq!(
            w.check(1, &Command::Detach { organization: 1 }),
            Err(Refused::Unable)
        );
        w.apply(1, Command::Attach { organization: 1 }).unwrap();
        assert_eq!(w.organizations[1].hq, Some(50));
        assert_eq!(w.attached(50).collect::<Vec<_>>(), [1]);
        // No other HQ to step on to.
        assert_eq!(
            w.check(1, &Command::Attach { organization: 1 }),
            Err(Refused::Unable)
        );
        // A second one, nearer: a detached organization takes the nearest, an
        // attached one the next round.
        let mut t = truck();
        (t.id, t.owner) = (70, 1);
        t.members[0].id = 70;
        t.place([26.0, 24.0], [0.0, 1.0]);
        w.add_organization(&t);
        w.apply(1, Command::Attach { organization: 1 }).unwrap();
        assert_eq!(w.organizations[1].hq, Some(70));
        assert!(w.detached(1).is_empty());
        w.notices.clear();
        w.apply(1, Command::Attach { organization: 1 }).unwrap();
        assert_eq!(w.organizations[1].hq, Some(50));
        // The player's told of each, a move as leaving then joining.
        let (player, organization) = (1, 1);
        let left = |hq| Notice::Detached {
            player,
            hq,
            organization,
        };
        let joined = |hq| Notice::Attached {
            player,
            hq,
            organization,
        };
        assert_eq!(w.notices, [left(70), joined(50)]);
        w.apply(1, Command::Detach { organization: 1 }).unwrap();
        assert_eq!(w.notices[2..], [left(50)]);
        w.apply(1, Command::Attach { organization: 1 }).unwrap();
        assert_eq!(w.organizations[1].hq, Some(70));
        // Not an enemy's, nor one that doesn't recruit its kind.
        w.organizations[1].hq = None;
        w.organizations[0].owner = 2;
        w.rules.recruit_lists.clear();
        assert_eq!(w.attaches_to(1), None);
    }

    #[test]
    fn organizations_no_hq_recruits_are_detached_too() {
        let mut w = division();
        // Agents, which nothing recruits, and a truck outside any
        // organization.
        w.rules
            .logistics
            .insert(Id::from("agents"), logistics(None));
        let mut agents = raised_company(&mut w, 1);
        agents.renumber(80);
        agents.template = "agents".into();
        agents.place([20.0, 20.0], [0.0, 1.0]);
        w.add_organization(&agents);
        let mut lorry = truck();
        (lorry.id, lorry.owner, lorry.template) = (90, 1, "lorry".into());
        lorry.members[0].id = 90;
        lorry.place([22.0, 30.0], [0.0, 1.0]);
        w.add_organization(&lorry);
        assert_eq!(w.detached(1), [1, 2]);
        // Only the company is one an HQ could take.
        assert!(w.attachable(1));
        w.apply(1, Command::Attach { organization: 1 }).unwrap();
        assert_eq!(w.detached(1), [2]);
        assert!(!w.attachable(1));
        // Attached or not, the company's kind is one an HQ takes.
        let hq = &w.hqs()[0];
        assert!(w.suits(hq, 1) && !w.suits(hq, 2) && !w.suits(hq, 3));
    }

    #[test]
    fn organizations_lose_their_hq_when_its_destroyed() {
        let mut w = division();
        w.apply(1, Command::Attach { organization: 1 }).unwrap();
        run(&mut w, 1.0);
        assert!(w.organizations[1].in_supply);
        w.destroy(0);
        run(&mut w, 1.0);
        assert_eq!(w.organizations[1].hq, None);
        assert!(!w.organizations[1].in_supply);
    }

    #[test]
    fn organizations_that_change_side_keep_their_place_at_their_hq() {
        let mut w = supplied_division();
        let hq = w.hqs().remove(0);
        // With an ally, its old side's zones are still its own.
        w.change_side(1, 3);
        run(&mut w, 1.0);
        assert_eq!(w.organizations[1].hq, Some(50));
        assert!(w.organizations[1].in_supply);
        assert_eq!(w.free_places(&hq), 1);
        // With an enemy it's still attached, but needs that side's zones.
        w.change_side(1, 2);
        run(&mut w, 1.0);
        assert_eq!(w.organizations[1].hq, Some(50));
        assert!(!w.organizations[1].in_supply);
        assert_eq!(w.free_places(&hq), 1);
        // An HQ that changes side loses its organizations at once.
        w.change_side(0, 3);
        assert_eq!(w.organizations[1].hq, None);
    }

    #[test]
    fn captured_hqs_lose_their_organizations_at_once() {
        let mut w = division();
        let hq = Command::Unpack {
            organization: 0,
            at: [30.0, 40.0],
            facing: NORTH,
        };
        w.apply(1, hq).unwrap();
        run(&mut w, 30.0);
        let b = w.organizations[0].unpacked_into.expect("unpacked");
        w.apply(1, Command::Attach { organization: 1 }).unwrap();
        w.change_hands(b, 2);
        assert_eq!(w.organizations[1].hq, None);
    }
}
