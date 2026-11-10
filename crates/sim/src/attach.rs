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
        for (r, regiment) in self.regiments.iter().enumerate() {
            let Some(places) = self.places(&regiment.template) else {
                continue;
            };
            // Unpacked, it's where its building is, and whose that is.
            let unpacked = (regiment.unpacked_into)
                .map(|b| &self.buildings[b])
                .filter(|b| !b.gone());
            let (owner, building, position) = match (unpacked, &regiment.unpacks) {
                (Some(b), _) => (b.owner, b.template.clone(), b.position),
                (None, Some(u)) if !self.destroyed(r) => {
                    (regiment.owner, u.building.template.clone(), self.centre(r))
                }
                _ => continue,
            };
            out.push(Hq {
                id: regiment.id,
                owner,
                building,
                position,
                places,
            });
        }
        // Those a map has unpacked, standing in their organizations.
        for b in self.buildings.iter().filter(|b| !b.gone()) {
            let within = b.organization.as_ref();
            if let Some((o, places)) = within.and_then(|o| Some((o, self.places(&o.template)?))) {
                out.push(Hq {
                    id: o.id,
                    owner: b.owner,
                    building: b.template.clone(),
                    position: b.position,
                    places,
                });
            }
        }
        out.sort_by_key(|h| h.id);
        out
    }

    /// The HQ the building at `b` is, if it's one.
    pub fn hq_of(&self, b: usize) -> Option<u32> {
        let building = &self.buildings[b];
        let (id, template) = match (&building.organization, building.unpacked_from) {
            (Some(o), _) => (o.id, o.template.as_str()),
            (None, Some(r)) => (self.regiments[r].id, self.regiments[r].template.as_str()),
            (None, None) => return None,
        };
        self.places(template).map(|_| id)
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

    /// Detaches the regiment at `r` if its HQ is gone or has changed side.
    pub(crate) fn keep_attached(&mut self, r: usize) {
        let regiment = &self.regiments[r];
        let Some(id) = regiment.hq else {
            return;
        };
        let stands = |h: &Hq| h.id == id && h.owner == regiment.owner;
        if !self.hqs().iter().any(stands) {
            self.regiments[r].hq = None;
        }
    }
}
