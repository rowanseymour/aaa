//! What things are to target filters: the target types each has, worked
//! out afresh at every test so they follow whatever it holds and is.

use std::collections::HashMap;

use tgmap::{Id, Targets};

use crate::{Relation, Target, World};

/// The types the game makes itself, in its own order, before the data's.
const MADE: [&str; 16] = [
    "friend",
    "enemy",
    "neutral",
    "independent",
    "own",
    "self",
    "same_organization",
    "constructable",
    "cannot_build",
    "organization",
    "under_construction",
    "capturable",
    "body",
    "actor",
    "invulnerable",
    "upgrading",
];

/// How many types a thing can hold: types further on share the slots of
/// those before them.
const SLOTS: usize = 32;

/// Which target types share a slot, so that holding one is holding them
/// all.
#[derive(Debug, Clone, Default)]
pub struct TypeSlots {
    /// The others in each type's slot, for those that share one.
    alike: HashMap<Id, Vec<Id>>,
}

impl TypeSlots {
    /// The slots of the game's own types and then, in turn, each new one
    /// of those `defined` in the order they load.
    pub fn new(defined: &[Id]) -> TypeSlots {
        let mut order: Vec<Id> = MADE.map(Id::from).to_vec();
        for t in defined {
            if !order.contains(t) {
                order.push(t.clone());
            }
        }
        let mut alike: HashMap<Id, Vec<Id>> = HashMap::new();
        for (i, a) in order.iter().enumerate() {
            for b in order.iter().skip(i + SLOTS).step_by(SLOTS) {
                alike.entry(a.clone()).or_default().push(b.clone());
                alike.entry(b.clone()).or_default().push(a.clone());
            }
        }
        TypeSlots { alike }
    }

    /// `name` and the types sharing its slot.
    fn alike<'a>(&'a self, name: &'a Id) -> impl Iterator<Item = &'a Id> {
        std::iter::once(name).chain(self.alike.get(name).into_iter().flatten())
    }
}

/// Something a filter can be tested on: a regiment as well as the units
/// and buildings that fight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Thing {
    Organization(usize),
    Unit(usize),
    Building(usize),
}

impl From<Target> for Thing {
    fn from(t: Target) -> Thing {
        match t {
            Target::Unit(u) => Thing::Unit(u),
            Target::Building(b) => Thing::Building(b),
        }
    }
}

/// Whose a filter is: a side's, and what of its tests it, if anything does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct User {
    pub player: u32,
    pub thing: Option<Target>,
}

impl User {
    /// A side's own, as a special operation is.
    pub(crate) fn side(player: u32) -> User {
        User {
            player,
            thing: None,
        }
    }
}

impl World {
    /// Whether `t` is what `targets` asks for, as `user` sees it: it has
    /// every type required and none excluded, holding one when it holds
    /// any that shares its slot. Each is looked into only as
    /// far as the filter asks, as most ask only how the sides stand.
    pub(crate) fn reaches(&self, user: User, targets: &Targets, t: impl Into<Thing>) -> bool {
        let t = t.into();
        let mut held = None;
        let mut holds = |name: &Id| {
            self.is_type(user, t, name).unwrap_or_else(|| {
                held.get_or_insert_with(|| self.held_types(t))
                    .contains(name)
            })
        };
        let slots = &self.rules.type_slots;
        let mut has = |name: &Id| slots.alike(name).any(&mut holds);
        targets.required.iter().all(&mut has) && !targets.excluded.iter().any(has)
    }

    /// Whether `t` has `name` as `user` sees it, for a type that comes of
    /// how its side stands to `user`'s or of what it is; `None` for one
    /// only properties give.
    fn is_type(&self, user: User, t: Thing, name: &Id) -> Option<bool> {
        let standing = || {
            let owner = self.thing_owner(t);
            let side = self.sides.player(owner);
            (side, side.map(|_| self.sides.relation(user.player, owner)))
        };
        let regiment = |of: Thing| self.regiment_of(of);
        let is = |n: &str| name.as_str().eq_ignore_ascii_case(n);
        let relation = || standing().1;
        Some(match () {
            _ if is("own") => matches!(relation(), Some(Relation::Itself)),
            _ if is("friend") => matches!(relation(), Some(Relation::Itself | Relation::Ally)),
            _ if is("enemy") => matches!(relation(), Some(Relation::Enemy)),
            _ if is("neutral") => matches!(relation(), None | Some(Relation::Neutral)),
            _ if is("independent") => standing().0.is_none_or(|p| p.independent),
            _ if is("self") => user.thing.is_some_and(|u| Thing::from(u) == t),
            _ if is("same_organization") => (user.thing)
                .and_then(|u| regiment(u.into()))
                .is_some_and(|r| regiment(t) == Some(r)),
            _ if is("actor") => true,
            _ if is("organization") => matches!(t, Thing::Organization(_)),
            _ if is("body") => self.has_body(t),
            _ if is("capturable") => self.capturable_now(t),
            _ if is("cannot_build") => !self.builds(user, t),
            _ if is("under_construction") => {
                matches!(t, Thing::Building(b) if self.buildings[b].constructing)
            }
            _ => return None,
        })
    }

    /// The types the properties `t` holds give it: its template's own,
    /// and what its regiment, if it's one or in one, holds and hands down
    /// to its members, its formation and its state in a fight included.
    fn held_types(&self, t: Thing) -> Vec<Id> {
        let table = &self.rules.property_types;
        let of = |p: &Id| {
            table
                .get(p)
                .into_iter()
                .flat_map(|e| e.types.iter().cloned())
        };
        let regiment = |r: usize, template: &Id| {
            let organization = &self.organizations[r];
            let (owner, held) = (organization.owner, &organization.properties);
            let kind = self.rules.kinds.get(&organization.template);
            let own = kind.into_iter().flat_map(|k| k.flags.iter().cloned());
            let bearing = self.bearing(table, |e| &e.requires, owner, template, held);
            let state = self.state(r).map(Id::from);
            let fight = self.fight_state(r).map(Id::from);
            let more = [self.level_property(r).cloned(), state, fight];
            let more = more
                .into_iter()
                .flatten()
                .flat_map(|p| of(&p).collect::<Vec<_>>());
            own.chain(bearing.flat_map(|e| e.types.iter().cloned()))
                .chain(self.formation_types(r).iter().cloned())
                .chain(more)
                .collect::<Vec<_>>()
        };
        match t {
            Thing::Organization(r) => regiment(r, &self.organizations[r].template),
            Thing::Unit(u) => {
                let unit = &self.units[u];
                let own = unit.fighter.iter().flat_map(|f| f.flags.iter().cloned());
                let timed = (unit.properties.iter()).filter_map(|h| table.get(&h.property));
                own.chain(timed.flat_map(|e| e.types.iter().cloned()))
                    .chain(regiment(unit.organization, &unit.template))
                    .collect()
            }
            Thing::Building(b) => {
                let building = &self.buildings[b];
                let (owner, held) = (building.owner, &building.properties);
                let bearing = self.bearing(table, |e| &e.requires, owner, &building.template, held);
                let handed = self.regiment_of(t).map(|r| regiment(r, &building.template));
                (building.fighter.flags.iter().cloned())
                    .chain(bearing.flat_map(|e| e.types.iter().cloned()))
                    .chain(handed.into_iter().flatten())
                    .collect()
            }
        }
    }

    /// Whether `user` can build things of `t`'s kind: a side on its own
    /// builds nothing.
    fn builds(&self, user: User, t: Thing) -> bool {
        let fighter = match user.thing {
            Some(Target::Unit(u)) => self.units[u].fighter.as_ref(),
            Some(Target::Building(b)) => Some(&self.buildings[b].fighter),
            None => None,
        };
        let template = match t {
            Thing::Organization(r) => &self.organizations[r].template,
            Thing::Unit(u) => &self.units[u].template,
            Thing::Building(b) => &self.buildings[b].template,
        };
        fighter.is_some_and(|f| f.builds.contains(template))
    }

    pub(crate) fn thing_owner(&self, t: Thing) -> u32 {
        match t {
            Thing::Organization(r) => self.organizations[r].owner,
            Thing::Unit(u) => self.organizations[self.units[u].organization].owner,
            Thing::Building(b) => self.buildings[b].owner,
        }
    }

    /// The regiment `t` is, or is one of, if any: a building is one of the
    /// regiment unpacked into it.
    pub(crate) fn regiment_of(&self, t: Thing) -> Option<usize> {
        match t {
            Thing::Organization(r) => Some(r),
            Thing::Unit(u) => Some(self.units[u].organization),
            Thing::Building(b) => (0..self.organizations.len())
                .find(|&r| self.organizations[r].unpacked_into == Some(b)),
        }
    }

    /// Whether `t` has health: a living unit or a building with some left.
    fn has_body(&self, t: Thing) -> bool {
        match t {
            Thing::Organization(_) => false,
            Thing::Unit(u) => self.units[u].fighter.is_some() && self.units[u].alive(),
            Thing::Building(b) => self.buildings[b].health > 0.0,
        }
    }

    /// Whether `t` could be captured by damage now: one that can be, with
    /// health, and neither a city's centre nor one of a standing city's
    /// buildings.
    fn capturable_now(&self, t: Thing) -> bool {
        match t {
            Thing::Organization(_) => false,
            Thing::Unit(u) => {
                let unit = &self.units[u];
                unit.alive() && unit.fighter.as_ref().is_some_and(|f| f.capturable)
            }
            Thing::Building(b) => {
                let building = &self.buildings[b];
                let city = building.zone.is_some() || self.city_of(b).is_some();
                building.health > 0.0 && building.fighter.capturable && !city
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mind;
    use crate::testing::*;

    /// Whether, to player 1's first unit, `t` is all of `types`.
    fn is(w: &World, t: Thing, types: &str) -> bool {
        let user = w.user(Target::Unit(0));
        let required = types.split(' ').map(Id::from).collect();
        let targets = Targets {
            required,
            excluded: Vec::new(),
        };
        w.reaches(user, &targets, t)
    }

    #[test]
    fn a_things_side_and_who_it_is_count_as_seen_by_the_user() {
        let mut sides = at_war();
        sides.players[1].independent = true;
        let mut w = battle(
            &[1, 1, 2, 3],
            vec![
                soldier(vec![]),
                soldier(vec![]),
                soldier(vec![]),
                soldier(vec![]),
            ],
            sides,
        );
        // A second member of the user's own regiment.
        w.units[1].organization = 0;
        w.organizations[0].units.push(1);
        assert!(is(
            &w,
            Thing::Unit(0),
            "self own friend same_organization actor body"
        ));
        assert!(is(&w, Thing::Unit(1), "own friend same_organization"));
        assert!(!is(&w, Thing::Unit(1), "self"));
        assert!(is(&w, Thing::Unit(2), "enemy independent"));
        assert!(is(&w, Thing::Unit(3), "friend"));
        assert!(!is(&w, Thing::Unit(3), "own") && !is(&w, Thing::Unit(3), "independent"));
        assert!(is(
            &w,
            Thing::Organization(0),
            "organization same_organization"
        ));
        assert!(!is(&w, Thing::Organization(0), "body"));
        w.units[2].die(0);
        assert!(!is(&w, Thing::Unit(2), "body"));
    }

    #[test]
    fn what_a_regiment_holds_its_members_count_as_too() {
        let mut w = battle(&[1, 2], vec![soldier(vec![]), soldier(vec![])], at_war());
        let types = |t: &str| tgmap::PropertyTypes {
            types: vec![Id::from(t)],
            requires: Vec::new(),
        };
        let rules = &mut w.rules;
        rules
            .property_types
            .insert(tgmap::ROUTING.into(), types("routing"));
        rules
            .property_types
            .insert("sabotage".into(), types("sabotaged"));
        assert!(!is(&w, Thing::Unit(1), "routing"));
        w.organizations[1].mind = Mind::Routing {
            to: None,
            until: 0,
            retry: 0,
        };
        assert!(is(&w, Thing::Unit(1), "routing"));
        assert!(is(&w, Thing::Organization(1), "routing"));
        w.give(crate::Piece::Organization(1), "sabotage".into(), 10.0);
        assert!(is(&w, Thing::Unit(1), "routing sabotaged infantry"));
    }

    #[test]
    fn a_building_is_capturable_while_it_has_health_and_isnt_a_citys() {
        let mut house = building(9, 2, [40.0, 10.0]);
        house.fighter.capturable = true;
        let w = with_buildings(battle(&[1], vec![soldier(vec![])], at_war()), &[house]);
        let mut w = w;
        assert!(is(&w, Thing::Building(0), "capturable body building enemy"));
        w.buildings[0].health = 0.0;
        assert!(!is(&w, Thing::Building(0), "capturable"));
    }

    #[test]
    fn a_citys_buildings_arent_capturable_while_it_stands() {
        let mut buildings = city();
        buildings[1].fighter.capturable = true;
        let w = with_buildings(battle(&[1], vec![soldier(vec![])], at_war()), &buildings);
        assert!(w.city_of(1).is_some());
        assert!(!is(&w, Thing::Building(1), "capturable"));
    }

    #[test]
    fn a_building_in_a_regiment_has_what_the_regiment_hands_down() {
        let w = battle(&[1], vec![soldier(vec![])], at_war());
        let mut w = with_buildings(w, &[building(9, 1, [30.0, 10.0])]);
        let types = tgmap::PropertyTypes {
            types: vec![Id::from("routing")],
            requires: Vec::new(),
        };
        w.rules.property_types.insert(tgmap::ROUTING.into(), types);
        w.organizations[0].mind = Mind::Routing {
            to: None,
            until: 0,
            retry: 0,
        };
        assert!(!is(&w, Thing::Building(0), "routing"));
        w.organizations[0].unpacked_into = Some(0);
        assert!(is(&w, Thing::Building(0), "routing same_organization"));
    }

    #[test]
    fn what_the_user_cant_build_is_cannot_build_to_it() {
        let w = battle(&[1], vec![soldier(vec![])], at_war());
        let mut w = with_buildings(w, &[building(9, 1, [30.0, 10.0])]);
        w.buildings[0].template = "bunker".into();
        assert!(is(&w, Thing::Building(0), "cannot_build"));
        let engineer = w.units[0].fighter.as_mut().unwrap();
        engineer.builds.push("bunker".into());
        assert!(!is(&w, Thing::Building(0), "cannot_build"));
        assert!(is(&w, Thing::Unit(0), "cannot_build"));
    }

    #[test]
    fn a_regiments_formation_hands_its_types_down() {
        let mut w = battle(&[1, 2], vec![soldier(vec![]), soldier(vec![])], at_war());
        let regiment = &w.organizations[1];
        let formation = &regiment.formations[regiment.current_formation()].id;
        let (formation, template) = (Id::from(formation.as_str()), regiment.template.clone());
        let rules = &mut w.rules;
        rules
            .formation_types
            .insert(formation, vec!["drilled".into()]);
        rules
            .broken_types
            .insert(template, vec!["scattered".into()]);
        assert!(is(&w, Thing::Unit(1), "drilled") && !is(&w, Thing::Unit(1), "scattered"));
        assert!(is(&w, Thing::Organization(1), "drilled"));
        w.organizations[1].mind = Mind::Routing {
            to: None,
            until: 0,
            retry: 0,
        };
        assert!(is(&w, Thing::Unit(1), "scattered") && !is(&w, Thing::Unit(1), "drilled"));
    }

    #[test]
    fn a_type_past_the_last_slot_shares_the_first_ones() {
        let defined: Vec<Id> = (0..17)
            .map(|i| Id::from(format!("t{i}").as_str()))
            .collect();
        let mut w = battle(&[1, 2], vec![soldier(vec![]), soldier(vec![])], at_war());
        w.rules.type_slots = TypeSlots::new(&defined);
        // The last of 33 takes the first's slot: `friend`'s.
        w.units[1]
            .fighter
            .as_mut()
            .unwrap()
            .flags
            .push("t16".into());
        assert!(is(&w, Thing::Unit(1), "friend enemy"));
        assert!(is(&w, Thing::Unit(0), "t16"));
        assert!(!is(&w, Thing::Unit(1), "t15"));
        // One defined again, here a type the game makes, takes no new slot.
        let again: Vec<Id> = ["friend".into()].into_iter().chain(defined).collect();
        w.rules.type_slots = TypeSlots::new(&again);
        assert!(is(&w, Thing::Unit(1), "friend"));
        w.rules.type_slots = TypeSlots::new(&again[..17]);
        assert!(!is(&w, Thing::Unit(1), "friend"));
    }
}
