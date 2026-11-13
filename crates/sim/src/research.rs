//! What a side has unlocked: the technologies it holds and researches, and
//! what they and its buildings let a recruiter offer.

use aaa_map::{Grant, Id, Research};

use crate::{Held, Notice, Refused, World};

/// What a recruiter would need to raise a regiment but hasn't, and so
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

/// For good, as a researched property never runs out.
const KEPT: u64 = u64::MAX;

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
    /// the map bars it.
    pub fn grant(&mut self, player: u32, technology: Id) {
        if self.barred(player, &technology) {
            return;
        }
        let held = self.technologies.entry(player).or_default();
        if !held.contains(&technology) {
            held.push(technology);
        }
    }

    /// Whether `player`'s side holds `technology`.
    pub fn holds(&self, player: u32, technology: &Id) -> bool {
        self.technologies
            .get(&player)
            .is_some_and(|t| t.contains(technology))
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
    /// unpacking, and its own rather than an ally's.
    fn stands(&self, player: u32, ids: &[Id]) -> bool {
        self.buildings
            .iter()
            .any(|b| b.owner == player && b.standing() && !b.busy() && ids.contains(&b.template))
    }

    /// Of `structures`, each building by every id it goes by, the first id
    /// of each `player` lacks: none where `any` and one stands.
    fn missing(&self, player: u32, structures: &[Vec<Id>], any: bool) -> Vec<Id> {
        let missing: Vec<Id> = (structures.iter())
            .filter(|ids| !self.stands(player, ids))
            .map(|ids| ids[0].clone())
            .collect();
        match any && missing.len() < structures.len() {
            true => Vec::new(),
            false => missing,
        }
    }

    /// What the building at `building` would need to raise `regiment` for
    /// `player`, checked member by member: a member missing a property it
    /// hides for leaves the regiment out, and where members differ the
    /// later one decides.
    pub fn lacks(&self, player: u32, building: usize, regiment: &aaa_map::Regiment) -> Lacking {
        let mut out = Lacking::default();
        let templates = std::iter::once(regiment.template.as_str())
            .chain(regiment.members.iter().map(|m| m.template.as_str()));
        out.barred = templates
            .into_iter()
            .any(|t| self.barred(player, &Id::from(t)));
        out.hidden = out.barred;
        for m in &regiment.members {
            let Some(r) = self.rules.requires.get(&Id::from(m.template.as_str())) else {
                continue;
            };
            let structures = self.missing(player, &r.structures, r.any);
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
        match self.missing(player, &item.structures, item.structures_any) {
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
    /// so when it's new to them.
    pub fn gain(&mut self, player: u32, technology: Id) {
        if self.holds(player, &technology) {
            return;
        }
        self.grant(player, technology.clone());
        // A barred one isn't granted.
        if self.holds(player, &technology) {
            self.notices.push(Notice::Gained { player, technology });
        }
    }

    /// Gives what the building at `building` has researched to whom its
    /// item says.
    pub(crate) fn researched(&mut self, building: usize, item: &Research) {
        let player = self.buildings[building].owner;
        let technology = item.technology.clone();
        let held = Held {
            property: technology.clone(),
            until: KEPT,
        };
        self.notices.push(Notice::Researched {
            player,
            building,
            technology: technology.clone(),
        });
        match item.grant {
            Grant::Side => self.gain(player, technology),
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

    use aaa_map::Id;

    use crate::{Command, Commission, Notice, Refused, Researchable, World};

    /// The economy's HQ listing two technologies for its side, 60 and 100
    /// money, the second needing the first and a depot.
    fn laboratory() -> World {
        let mut w = economy();
        let item = |technology: &str, money| aaa_map::Research {
            technology: Id::from(technology),
            grant: aaa_map::Grant::Side,
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
        assert!(matches!(
            told[..],
            [
                Notice::Researched {
                    player: 1,
                    building: 0,
                    ..
                },
                Notice::Gained { player: 1, .. }
            ]
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
    fn research_waits_its_turn_in_the_queue_and_is_refunded_if_called_off() {
        let mut w = laboratory();
        w.apply(
            1,
            Command::Commission {
                building: 0,
                commission: Box::new(Commission::new(truck(), 1.0)),
            },
        )
        .unwrap();
        w.apply(1, research("tech_heavy")).unwrap();
        assert_eq!(w.stockpile(1), 190.0);
        assert_eq!(w.buildings[0].queue[1].template(), "tech_heavy");
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
        list[0].grant = aaa_map::Grant::Building;
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
        list[0].grant = aaa_map::Grant::Settlement;
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
        let order = Command::Commission {
            building: 0,
            commission: Box::new(Commission::new(truck(), 1.0)),
        };
        assert_eq!(w.apply(1, order), Err(Refused::Lacking));
        // Only for the side barred.
        w.grant(2, Id::from("tech_heavy"));
        assert!(w.holds(2, &Id::from("tech_heavy")));
    }
}
