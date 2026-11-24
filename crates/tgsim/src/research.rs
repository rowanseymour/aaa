//! What a side has unlocked: the technologies it holds and researches, and
//! what they and its buildings let a recruiter offer.

use std::collections::HashSet;

use tgmap::{Grant, Id, Research};

use crate::properties::KEPT;
use crate::{Building, Held, Notice, Piece, Refused, State, World};

/// What a recruiter would need to raise an organization but hasn't, and so
/// whether its menu offers it, greys it or leaves it out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lacking {
    /// The buildings its owner hasn't standing, by their current ids.
    pub structures: Vec<Id>,
    /// The properties neither it nor, for a technology, its side has.
    pub properties: Vec<Id>,
    /// Whether it's left out of the menu, rather than greyed.
    pub hidden: bool,
    /// Whether the map bars its owner from it, which leaves it out too.
    pub barred: bool,
}

impl Lacking {
    /// Whether nothing's missing, so the recruiter offers it.
    pub fn is_empty(&self) -> bool {
        self.structures.is_empty() && self.properties.is_empty() && !self.barred
    }
}

/// How a building's research menu shows one of the technologies it lists.
#[derive(Debug, Clone, PartialEq)]
pub enum Researchable {
    Ready,
    /// Greyed, for want of these buildings, by their current ids.
    Lacking(Vec<Id>),
    /// Already researched, or being researched: its slot is left empty.
    Taken,
    /// Left out, the items after it taking its place.
    Hidden,
}

/// One of a side's grants of a technology. A side holds a technology
/// while it holds any grant of it, and each grant goes on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Granted {
    pub technology: Id,
    pub source: Source,
}

/// Where a grant came from, which decides what can take it away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The map's start technologies, the handicap's: never taken away.
    Kept,
    /// Research or a trigger, which a trigger can take away.
    Given,
    /// The building at this index into [`World::buildings`], for as long
    /// as the side holds it.
    Building(usize),
}

impl World {
    /// Whether the map bars `player` from a technology or template.
    pub fn barred(&self, player: u32, id: &Id) -> bool {
        (self.sides.player(player)).is_some_and(|p| {
            p.barred
                .iter()
                .any(|b| b.as_str().eq_ignore_ascii_case(id.as_str()))
        })
    }

    /// Gives `player` a technology, for all it has and will have, unless
    /// the map bars it, saying nothing: a start technology's grant, which
    /// nothing takes away.
    pub fn grant(&mut self, player: u32, technology: Id) {
        self.add_grant(player, technology, Source::Kept);
    }

    /// Adds a grant of `technology` from `source` to `player`'s, unless the
    /// map bars it. Returns whether the technology is new to the side.
    fn add_grant(&mut self, player: u32, technology: Id, source: Source) -> bool {
        if self.barred(player, &technology) {
            return false;
        }
        let new = !self.holds(player, &technology);
        let grants = self.technologies.entry(player).or_default();
        grants.push(Granted { technology, source });
        new
    }

    /// Takes away the first of `player`'s grants of `technology` that
    /// `takes` picks, telling them when it was the last. Returns whether
    /// there was one.
    fn take_grant(&mut self, player: u32, technology: &Id, takes: impl Fn(Source) -> bool) -> bool {
        let Some(grants) = self.technologies.get_mut(&player) else {
            return false;
        };
        let Some(i) = (grants.iter()).position(|g| g.technology == *technology && takes(g.source))
        else {
            return false;
        };
        let Granted { technology, .. } = grants.remove(i);
        if !self.holds(player, &technology) {
            self.notices
                .push(Notice::TechnologyLost { player, technology });
        }
        true
    }

    /// Whether `player`'s side holds `technology`, from any grant.
    pub fn holds(&self, player: u32, technology: &Id) -> bool {
        (self.technologies.get(&player))
            .is_some_and(|g| g.iter().any(|g| g.technology == *technology))
    }

    /// The technologies `player`'s side holds, each once, in the order it
    /// first held them.
    pub fn technologies_of(&self, player: u32) -> impl Iterator<Item = &Id> + Clone {
        let grants = self
            .technologies
            .get(&player)
            .map_or(&[][..], Vec::as_slice);
        (grants.iter().enumerate())
            .filter(|&(i, g)| !grants[..i].iter().any(|e| e.technology == g.technology))
            .map(|(_, g)| &g.technology)
    }

    /// The technologies `on`'s side holds that can be on it, each once, in
    /// the order the side first held them, with where that first grant
    /// came from: none for an independent side's. A regiment can hold one
    /// when any of its places can, a dead member's counting by its
    /// template, as its replacement will.
    pub fn technologies_on(&self, on: Piece) -> Vec<(&Id, Source)> {
        let (owner, templates): (u32, Vec<&Id>) = match on {
            Piece::Organization(r) => {
                let o = &self.organizations[r];
                let members = o.units.iter().map(|&u| &self.units[u].template);
                (
                    o.owner,
                    std::iter::once(&o.template).chain(members).collect(),
                )
            }
            Piece::Building(b) => (self.buildings[b].owner, vec![&self.buildings[b].template]),
        };
        let playing = (self.sides.player(owner)).is_some_and(|p| !p.independent);
        let grants = match playing {
            true => self.technologies.get(&owner).map_or(&[][..], Vec::as_slice),
            false => &[][..],
        };
        let kinds = &self.rules.kinds;
        let fits = |p: &Id| {
            let needs = self.rules.property_requires.get(p);
            let needs = needs.map_or(&[][..], Vec::as_slice);
            kinds.contains_key(templates[0])
                && (templates.iter().filter_map(|t| kinds.get(*t)))
                    .any(|k| needs.iter().all(|n| k.properties.contains(n)))
        };
        (grants.iter().enumerate())
            .filter(|&(i, g)| !grants[..i].iter().any(|e| e.technology == g.technology))
            .filter(|(_, g)| fits(&g.technology))
            .map(|(_, g)| (&g.technology, g.source))
            .collect()
    }

    /// The side the building at `b` gives its technologies to, if any:
    /// its owner from when it joins the side, its remains included, unless
    /// it's an independent one.
    fn grantee(&self, b: usize) -> Option<u32> {
        let building = &self.buildings[b];
        let there = match building.state {
            State::Destroyed(_) => (self.building_remains(b)).is_some_and(|r| self.ticks < r.gone),
            State::Packed(..) | State::Sold(_) => false,
            State::Standing | State::Disabled => true,
        };
        let sided = (self.sides.player(building.owner)).is_some_and(|p| !p.independent);
        (there && sided).then_some(building.owner)
    }

    /// Brings each side's grants from buildings in line with what it
    /// holds: one that's left the side takes its grants away first, then
    /// its new side gets them.
    pub(crate) fn grant_from_buildings(&mut self) {
        if self.rules.granted.is_empty() {
            return;
        }
        for b in 0..self.buildings.len() {
            let Some(list) = self.rules.granted.get(&self.buildings[b].template) else {
                continue;
            };
            let list = list.clone();
            let source = Source::Building(b);
            let had = (self.technologies.iter())
                .find(|(_, g)| g.iter().any(|g| g.source == source))
                .map(|(&p, _)| p);
            let has = self.grantee(b);
            if had == has {
                continue;
            }
            if let Some(p) = had {
                for t in &list {
                    self.take_grant(p, t, |s| s == source);
                }
            }
            if let Some(p) = has {
                for t in list {
                    self.gain_from(p, t, source);
                }
            }
        }
    }

    /// Whether the building at `building` has `property` of its own: its
    /// template's, or one it's been given.
    fn carries(&self, building: usize, property: &Id) -> bool {
        self.buildings.get(building).is_some_and(|b| {
            let kind = self.rules.kinds.get(&b.template);
            kind.is_some_and(|k| k.properties.contains(property))
                || b.properties.iter().any(|h| h.property == *property)
        })
    }

    /// Whether `player` has a building of one of `ids` standing: done
    /// unpacking, and its own rather than an ally's. For a settlement's
    /// centre at `building`, only its own settlement's count, and those
    /// in none.
    fn stands(&self, player: u32, building: usize, ids: &[Id]) -> bool {
        let centres: HashSet<u32> = self.settlements.iter().map(|s| s.centre).collect();
        let own = self.buildings.get(building).map(|b| b.id);
        let settled = own.is_some_and(|id| centres.contains(&id));
        let counts = |b: &Building| !settled || Some(b.id) == own || !centres.contains(&b.id);
        (self.buildings.iter()).any(|b| {
            b.owner == player && b.standing() && !b.busy() && ids.contains(&b.template) && counts(b)
        })
    }

    /// Of `structures`, each building by every id that meets the need for
    /// it, the first id of each `player` lacks: none where `any` and one
    /// stands.
    fn missing(&self, player: u32, building: usize, structures: &[Vec<Id>], any: bool) -> Vec<Id> {
        let missing: Vec<Id> = (structures.iter())
            .filter(|ids| !self.stands(player, building, ids))
            .map(|ids| ids[0].clone())
            .collect();
        match any && missing.len() < structures.len() {
            true => Vec::new(),
            false => missing,
        }
    }

    /// What the building at `building` would need to raise `organization` for
    /// `player`, checked member by member: a member missing a property it
    /// hides for leaves the organization out, and where members differ the
    /// later one decides.
    pub fn lacks(
        &self,
        player: u32,
        building: usize,
        organization: &tgmap::Organization,
    ) -> Lacking {
        let mut out = Lacking::default();
        let members = organization.members.iter().map(|m| &m.template);
        let mut templates = std::iter::once(&organization.template).chain(members);
        out.barred = templates.any(|t| self.barred(player, t));
        out.hidden = out.barred;
        for m in &organization.members {
            let Some(r) = self.rules.requires.get(&m.template) else {
                continue;
            };
            let structures = self.missing(player, building, &r.structures, r.any);
            let properties: Vec<&Id> = (r.properties.iter())
                .filter(|p| !self.carries(building, p) && !self.holds(player, p))
                .collect();
            if structures.is_empty() && properties.is_empty() {
                continue;
            }
            out.hidden = out.barred || (r.hide && !properties.is_empty());
            for id in structures {
                if !out.structures.contains(&id) {
                    out.structures.push(id);
                }
            }
            for id in properties {
                if !out.properties.contains(id) {
                    out.properties.push(id.clone());
                }
            }
        }
        out
    }

    /// What the building at `building` lists for research, in its menu's
    /// order.
    pub fn research_list(&self, building: usize) -> &[Research] {
        (self.buildings.get(building))
            .and_then(|b| self.rules.research_lists.get(&b.template))
            .map_or(&[], Vec::as_slice)
    }

    /// The technologies under way in any of `player`'s buildings.
    fn researching(&self, player: u32) -> Vec<&Id> {
        (self.buildings.iter().filter(|b| b.owner == player))
            .flat_map(|b| &b.queue)
            .filter_map(|c| match &c.work {
                crate::Work::Research(r) => Some(&r.technology),
                crate::Work::Raise(_) => None,
            })
            .collect()
    }

    /// How the building at `building` offers `item` to `player`.
    pub fn researchable(&self, player: u32, building: usize, item: &Research) -> Researchable {
        self.offer(player, building, item, &self.researching(player))
    }

    /// How the building at `building` offers each item it lists to
    /// `player`, in its menu's order.
    pub fn researchables(&self, player: u32, building: usize) -> Vec<Researchable> {
        let researching = self.researching(player);
        (self.research_list(building).iter())
            .map(|item| self.offer(player, building, item, &researching))
            .collect()
    }

    /// [`World::researchable`], given what's under way on the side.
    fn offer(
        &self,
        player: u32,
        building: usize,
        item: &Research,
        researching: &[&Id],
    ) -> Researchable {
        let Some(b) = self.buildings.get(building) else {
            return Researchable::Hidden;
        };
        let tech = &item.technology;
        let queued = |b: &crate::Building| {
            (b.queue.iter())
                .any(|c| matches!(&c.work, crate::Work::Research(r) if r.technology == *tech))
        };
        let taken = match item.grant {
            Grant::Side => self.holds(player, tech) || researching.contains(&tech),
            Grant::Building => self.carries(building, tech) || queued(b),
            // Nothing keeps it, so it's never done.
            Grant::Settlement => queued(b),
        };
        if taken {
            return Researchable::Taken;
        }
        if self.barred(player, tech) {
            return Researchable::Hidden;
        }
        let has = |p: &Id| self.carries(building, p) || self.holds(player, p);
        let properties = match item.properties_any && !item.properties.is_empty() {
            true => item.properties.iter().any(has),
            false => item.properties.iter().all(has),
        };
        if !properties {
            return Researchable::Hidden;
        }
        match self.missing(player, building, &item.structures, item.structures_any) {
            missing if missing.is_empty() => Researchable::Ready,
            missing => Researchable::Lacking(missing),
        }
    }

    /// The item for `technology` the building at `building` would research
    /// for `player` now, or why it won't.
    pub(crate) fn may_research(
        &self,
        player: u32,
        building: usize,
        technology: &Id,
    ) -> Result<&Research, Refused> {
        let b = self.standing_for(player, building)?;
        let item = (self.research_list(building).iter()).find(|r| r.technology == *technology);
        let Some(item) = item.filter(|_| !b.busy()) else {
            return Err(Refused::Unable);
        };
        match self.researchable(player, building, item) {
            Researchable::Ready => {}
            Researchable::Taken => return Err(Refused::Unable),
            Researchable::Lacking(_) | Researchable::Hidden => return Err(Refused::Lacking),
        }
        if self.research_cost(item) > self.stockpile(player) {
            return Err(Refused::Unaffordable);
        }
        Ok(item)
    }

    /// Gives `player` a technology as [`World::grant`] does, telling them
    /// so when it's new to them: a handicap's.
    pub fn gain(&mut self, player: u32, technology: Id) {
        self.gain_from(player, technology, Source::Kept);
    }

    /// Gives `player` a grant of `technology` from `source`, telling them
    /// when it's new to them.
    fn gain_from(&mut self, player: u32, technology: Id, source: Source) {
        if self.add_grant(player, technology.clone(), source) {
            self.notices.push(Notice::Gained { player, technology });
        }
    }

    /// Gives `player` a grant of `technology` that a trigger can take away
    /// again, as `kingdom_gains_technology` does.
    pub(crate) fn gain_taken_back(&mut self, player: u32, technology: Id) {
        self.gain_from(player, technology, Source::Given);
    }

    /// Takes away one of `player`'s grants of `technology` that research or
    /// a trigger gave, as `kingdom_loses_technology` does. Returns whether
    /// there was one.
    pub(crate) fn lose_given(&mut self, player: u32, technology: &Id) -> bool {
        self.take_grant(player, technology, |s| s == Source::Given)
    }

    /// Gives what the building at `building` has researched to whom its
    /// item says.
    pub(crate) fn researched(&mut self, building: usize, item: &Research) {
        let player = self.buildings[building].owner;
        let technology = item.technology.clone();
        let held = Held {
            property: technology.clone(),
            until: KEPT,
            by: None,
        };
        self.notices.push(Notice::Researched {
            player,
            building,
            technology: technology.clone(),
        });
        match item.grant {
            // Research says only that it's done.
            Grant::Side => {
                self.add_grant(player, technology, Source::Given);
            }
            Grant::Building => self.buildings[building].properties.push(held),
            // No building belongs to a settlement, so there's none to have
            // it.
            Grant::Settlement => {}
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::testing::*;

    use tgmap::{Domain, Id, Pathing};

    use super::Source;
    use crate::{Command, Notice, Refused, Researchable, State, World};

    /// The economy's HQ listing two technologies for its side, 60 and 100
    /// money, the second needing the first and a depot.
    fn laboratory() -> World {
        let mut w = economy();
        let item = |technology: &str, money| tgmap::Research {
            technology: Id::from(technology),
            grant: tgmap::Grant::Side,
            seconds: 1.0,
            cost: amounts(&[("money", money)]),
            structures: Vec::new(),
            structures_any: false,
            properties: Vec::new(),
            properties_any: false,
        };
        let mut tiger = item("tech_tiger", 100.0);
        tiger.properties = vec![Id::from("tech_heavy")];
        tiger.structures = vec![vec![Id::from("depot")]];
        (w.rules.research_lists).insert(Id::from("house"), vec![item("tech_heavy", 60.0), tiger]);
        w
    }

    fn research(technology: &str) -> Command {
        Command::Research {
            building: 0,
            technology: Id::from(technology),
        }
    }

    #[test]
    fn research_is_paid_for_up_front_and_gives_the_side_its_technology() {
        let mut w = laboratory();
        let (heavy, tiger) = (w.rules.research_lists[&Id::from("house")][0].clone(), {
            w.rules.research_lists[&Id::from("house")][1].clone()
        });
        assert_eq!(w.researchable(1, 0, &heavy), Researchable::Ready);
        // What needs a technology the side hasn't is left out.
        assert_eq!(w.researchable(1, 0, &tiger), Researchable::Hidden);
        assert_eq!(w.apply(1, research("tech_tiger")), Err(Refused::Lacking));
        assert_eq!(w.apply(1, research("tech_none")), Err(Refused::Unable));
        // Only an enemy's building would research for them.
        assert_eq!(w.apply(2, research("tech_heavy")), Err(Refused::NotYours));
        w.apply(1, research("tech_heavy")).unwrap();
        assert_eq!(w.stockpile(1), 440.0);
        // Once under way, it's not offered again.
        assert_eq!(w.researchable(1, 0, &heavy), Researchable::Taken);
        assert_eq!(w.apply(1, research("tech_heavy")), Err(Refused::Unable));
        for _ in 0..10 {
            assert!(!w.holds(1, &Id::from("tech_heavy")));
            w.tick();
        }
        assert!(w.holds(1, &Id::from("tech_heavy")));
        let told: Vec<&Notice> = (w.notices.iter())
            .filter(|n| matches!(n, Notice::Researched { .. } | Notice::Gained { .. }))
            .collect();
        // Research says only that it's done.
        assert!(matches!(
            told[..],
            [Notice::Researched {
                player: 1,
                building: 0,
                ..
            }]
        ));
        assert_eq!(w.researchable(1, 0, &heavy), Researchable::Taken);
        // A building it needs only greys it.
        assert_eq!(
            w.researchable(1, 0, &tiger),
            Researchable::Lacking(vec![Id::from("depot")])
        );
        assert_eq!(w.apply(1, research("tech_tiger")), Err(Refused::Lacking));
    }

    #[test]
    fn things_hold_their_sides_technologies_that_can_be_on_them_once_each() {
        let mut w = siege(&[building(9, 2, [10.0, 20.0])]);
        w.rules.kinds.insert("house".into(), kind(&["walls"]));
        let needs = |w: &mut World, p: &str, on: &str| {
            (w.rules.property_requires).insert(Id::from(p), vec![Id::from(on)]);
        };
        needs(&mut w, "tech_mortar", "walls");
        needs(&mut w, "tech_diesel", "tank");
        let grant = |technology: &str, source| crate::Granted {
            technology: Id::from(technology),
            source,
        };
        let grants = vec![
            grant("tech_diesel", Source::Given),
            grant("tech_drop", Source::Building(3)),
            grant("tech_mortar", Source::Given),
            grant("tech_drop", Source::Given),
        ];
        w.technologies.insert(2, grants);
        let on = w.technologies_on(crate::Piece::Building(0));
        let drop = Id::from("tech_drop");
        let mortar = Id::from("tech_mortar");
        // A house has no tank, and a second grant counts as its first.
        assert_eq!(
            on,
            vec![(&drop, Source::Building(3)), (&mortar, Source::Given)]
        );
        // An independent side's things hold none.
        let side = w.sides.players.iter_mut().find(|p| p.id == 2).unwrap();
        side.independent = true;
        assert!(w.technologies_on(crate::Piece::Building(0)).is_empty());
    }

    #[test]
    fn buildings_give_their_side_technologies_while_it_holds_them() {
        let mut w = World::from_organizations(at_war(), Pathing::uniform(40, Domain::Land), &[]);
        let parts = |id, owner, x| {
            let mut b = building(id, owner, [x, 10.0]);
            b.template = "parts".into();
            b
        };
        w.add_buildings(&[parts(30, 1, 10.0), parts(31, 1, 20.0), parts(32, 4, 30.0)]);
        w.sides.players.push(player(4, None, &[0, 0, 0, 0], &[]));
        let line = Id::from("assembly_line");
        w.rules
            .granted
            .insert(Id::from("parts"), vec![line.clone()]);
        let told = |w: &World| {
            (w.notices.iter())
                .filter_map(|n| match n {
                    Notice::Gained { player, .. } => Some((*player, true)),
                    Notice::TechnologyLost { player, .. } => Some((*player, false)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        // Two grants, told once; an independent side's building gives none.
        w.tick();
        assert_eq!(w.technologies[&1].len(), 2);
        assert!(!w.holds(4, &line));
        assert_eq!(told(&w), [(1, true)]);
        // Changing hands, the old side keeps its other grant.
        w.change_hands(0, 2);
        w.tick();
        assert!(w.holds(1, &line) && w.holds(2, &line));
        assert_eq!(told(&w), [(1, true), (2, true)]);
        // The last grant goes with the building, told.
        w.buildings[1].state = State::Sold(w.ticks);
        w.tick();
        assert!(!w.holds(1, &line));
        assert_eq!(told(&w), [(1, true), (2, true), (1, false)]);
        // A destroyed one grants while its remains stand.
        w.rules.remains.insert(Id::from("parts"), [1.0, 1.0]);
        w.buildings[0].state = State::Destroyed(w.ticks);
        run(&mut w, 1.9);
        assert!(w.holds(2, &line));
        run(&mut w, 0.2);
        assert!(!w.holds(2, &line));
    }

    #[test]
    fn research_waits_its_turn_in_the_queue_and_is_refunded_if_called_off() {
        let mut w = laboratory();
        w.apply(1, commission(0, "truck")).unwrap();
        w.apply(1, research("tech_heavy")).unwrap();
        assert_eq!(w.stockpile(1), 190.0);
        assert_eq!(w.buildings[0].queue[1].template().as_str(), "tech_heavy");
        let researching = w.buildings[0].queue[1].id;
        w.apply(
            1,
            Command::CancelCommission {
                building: 0,
                commission: researching,
            },
        )
        .unwrap();
        assert_eq!(w.stockpile(1), 250.0);
        // Too little money left for it then.
        w.set_stockpile(1, 50.0);
        assert_eq!(
            w.apply(1, research("tech_heavy")),
            Err(Refused::Unaffordable)
        );
        // It waits behind the truck, which waits for room under a limit,
        // and gets on once that's called off.
        w.set_stockpile(1, 60.0);
        w.apply(1, research("tech_heavy")).unwrap();
        for _ in 0..30 {
            w.tick();
        }
        assert!(!w.holds(1, &Id::from("tech_heavy")));
        let truck = w.buildings[0].queue[0].id;
        let cancel = Command::CancelCommission {
            building: 0,
            commission: truck,
        };
        w.apply(1, cancel).unwrap();
        for _ in 0..10 {
            w.tick();
        }
        assert!(w.holds(1, &Id::from("tech_heavy")));
    }

    #[test]
    fn research_needs_no_deploy_point_and_a_building_can_keep_its_own() {
        let mut w = laboratory();
        w.buildings[0].deploy = None;
        let list = w.rules.research_lists.get_mut(&Id::from("house")).unwrap();
        list[0].grant = tgmap::Grant::Building;
        w.apply(1, research("tech_heavy")).unwrap();
        for _ in 0..10 {
            w.tick();
        }
        // The building has it, which is enough for what needs it there, but
        // the side doesn't.
        assert!(!w.holds(1, &Id::from("tech_heavy")));
        let tiger = w.rules.research_lists[&Id::from("house")][1].clone();
        assert!(matches!(
            w.researchable(1, 0, &tiger),
            Researchable::Lacking(_)
        ));
        assert!(
            w.notices
                .iter()
                .any(|n| matches!(n, Notice::Researched { .. }))
        );
        assert!(!w.notices.iter().any(|n| matches!(n, Notice::Gained { .. })));
        // Taking it away offers it again.
        let heavy = w.rules.research_lists[&Id::from("house")][0].clone();
        assert_eq!(w.researchable(1, 0, &heavy), Researchable::Taken);
        assert!(w.lose_own(crate::targets::Thing::Building(0), &Id::from("tech_heavy")));
        assert_eq!(w.researchable(1, 0, &heavy), Researchable::Ready);
    }

    #[test]
    fn a_side_researches_a_technology_in_one_building_at_a_time() {
        let mut w = laboratory();
        let mut second = building(6, 1, [5.0, 5.0]);
        second.deploy = w.buildings[0].deploy;
        w.add_buildings(&[second]);
        w.apply(1, research("tech_heavy")).unwrap();
        let heavy = w.rules.research_lists[&Id::from("house")][0].clone();
        assert_eq!(w.researchable(1, 1, &heavy), Researchable::Taken);
        // The enemy researches its own.
        assert_eq!(w.researchable(2, 1, &heavy), Researchable::Ready);
    }

    #[test]
    fn research_for_a_settlement_goes_nowhere_and_can_be_had_again() {
        let mut w = laboratory();
        let list = w.rules.research_lists.get_mut(&Id::from("house")).unwrap();
        list[0].grant = tgmap::Grant::Settlement;
        let heavy = list[0].clone();
        w.apply(1, research("tech_heavy")).unwrap();
        assert_eq!(w.researchable(1, 0, &heavy), Researchable::Taken);
        for _ in 0..10 {
            w.tick();
        }
        assert!(
            w.notices
                .iter()
                .any(|n| matches!(n, Notice::Researched { .. }))
        );
        assert!(!w.holds(1, &Id::from("tech_heavy")));
        assert!(w.buildings[0].properties.is_empty());
        assert_eq!(w.researchable(1, 0, &heavy), Researchable::Ready);
        let before = w.stockpile(1);
        w.apply(1, research("tech_heavy")).unwrap();
        assert_eq!(w.stockpile(1), before - 60.0);
    }

    #[test]
    fn the_map_bars_sides_from_technologies_and_recruits() {
        let mut w = laboratory();
        let side = w.sides.players.iter_mut().find(|p| p.id == 1).unwrap();
        side.barred = vec![Id::from("TECH_HEAVY"), Id::from("truck")];
        // A barred technology is left out of the menu and never gained.
        let heavy = w.rules.research_lists[&Id::from("house")][0].clone();
        assert_eq!(w.researchable(1, 0, &heavy), Researchable::Hidden);
        assert_eq!(w.apply(1, research("tech_heavy")), Err(Refused::Lacking));
        w.grant(1, Id::from("tech_heavy"));
        assert!(!w.holds(1, &Id::from("tech_heavy")));
        // Nor is a barred recruit offered.
        let lacking = w.lacks(1, 0, &truck());
        assert!(lacking.barred && lacking.hidden && !lacking.is_empty());
        let order = commission(0, "truck");
        assert_eq!(w.apply(1, order), Err(Refused::Lacking));
        // Only for the side barred.
        w.grant(2, Id::from("tech_heavy"));
        assert!(w.holds(2, &Id::from("tech_heavy")));
    }
}
