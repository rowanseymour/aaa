//! Companies: the organizations a Kohan II settlement recruits.

use std::collections::{HashMap, HashSet};

use crate::{GameData, Id, Layout, Requires, defs};

/// What a thing's `recruitable` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recruitable {
    Yes,
    No,
    WithPrimary,
}

impl Recruitable {
    /// `default` where it says nothing, or something unknown.
    fn of(thing: &defs::Thing, default: Recruitable) -> Recruitable {
        match thing.recruitable.as_deref().map(str::to_ascii_lowercase) {
            Some(r) if r == "yes" => Recruitable::Yes,
            Some(r) if r == "no" => Recruitable::No,
            Some(r) if r == "with_primary" => Recruitable::WithPrimary,
            _ => default,
        }
    }
}

impl GameData {
    /// The company templates each settlement template can recruit, by the
    /// settlement's id, in the order they're defined: those recruitable
    /// whose every unit is one this kind of settlement could ever have the
    /// structures for. Whether a particular settlement has them yet is
    /// for the match to say.
    pub fn settlement_recruits(&self) -> HashMap<Id, Vec<Id>> {
        let definitions = self.definitions();
        let things: HashMap<&Id, &defs::Thing> = (definitions.things.iter())
            .map(|t| (&t.id, &t.thing))
            .collect();
        let layouts = self.layouts();
        let companies: Vec<(Id, Vec<Id>)> = (definitions.things.iter())
            .filter_map(|t| Some((t.id.clone(), company(&t.thing, &layouts, &things)?)))
            .collect();
        let requires = self.requirements();
        let upgrades = self.upgrades();
        (definitions.things.iter())
            .filter_map(|t| {
                let possible =
                    possible_structures(t.thing.settlement_component.as_ref()?, &upgrades);
                let list = (companies.iter())
                    .filter(|(_, units)| {
                        units.iter().all(|u| buildable(requires.get(u), &possible))
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                Some((t.id.clone(), list))
            })
            .collect()
    }
}

/// The units of `thing`'s design, for a company template that can be
/// recruited: an organization recruitable by its `recruitable` (by default
/// when its layout has a primary slot), whose every unit is a recruitable
/// element.
fn company(
    thing: &defs::Thing,
    layouts: &HashMap<Id, Layout>,
    things: &HashMap<&Id, &defs::Thing>,
) -> Option<Vec<Id>> {
    let o = thing.organization_component.as_ref()?;
    let layout = layouts.get(o.layout_ids.as_ref()?)?;
    let recruitable = match Recruitable::of(thing, Recruitable::WithPrimary) {
        Recruitable::Yes => true,
        Recruitable::No => false,
        Recruitable::WithPrimary => layout.primary.is_some(),
    };
    if !recruitable {
        return None;
    }
    // Templates name an empty slot's element `null`.
    let units: Vec<Id> = (o.slot.iter())
        .filter_map(|s| crate::not_null(s.element_ids.clone()))
        .collect();
    let element = |u: &Id| {
        things.get(u).is_some_and(|t| {
            t.element_component.is_some() && Recruitable::of(t, Recruitable::Yes) != Recruitable::No
        })
    };
    (!units.is_empty() && units.iter().all(element)).then_some(units)
}

/// Every structure a settlement could ever have: its centres at every
/// level, the structures it can build, and what those upgrade into.
fn possible_structures(
    settlement: &defs::SettlementComponent,
    upgrades: &HashMap<Id, Vec<Id>>,
) -> HashSet<Id> {
    let centres = crate::settlements::centres(settlement);
    let available = (settlement.available_structures.iter())
        .flat_map(|a| &a.structure)
        .filter_map(|s| s.structure_ids.clone());
    let mut out = HashSet::new();
    let mut todo: Vec<Id> = centres.into_iter().chain(available).collect();
    while let Some(s) = todo.pop() {
        if let Some(next) = upgrades.get(&s) {
            todo.extend(next.iter().filter(|n| !out.contains(*n)).cloned());
        }
        out.insert(s);
    }
    out
}

/// Whether the structures a unit requires are among `possible`, by any of
/// the ids each goes by: all of them, or one for a unit that needs any. A
/// unit with none passes everywhere.
fn buildable(requires: Option<&Requires>, possible: &HashSet<Id>) -> bool {
    requires.is_none_or(|r| {
        let mut structures = r.structures.iter();
        let found = |ids: &Vec<Id>| ids.iter().any(|id| possible.contains(id));
        match r.any && !r.structures.is_empty() {
            true => structures.any(found),
            false => structures.all(found),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thing(text: &str) -> defs::Thing {
        let parsed = tgi::parse(text);
        let files = [("units/test.tgi".to_string(), parsed)];
        let files = crate::scoped(&files);
        files[0].read::<defs::Thing>("Thing").next().unwrap()
    }

    fn layout(primary: Option<usize>) -> Layout {
        Layout {
            formations: Vec::new(),
            default: 0,
            combat: None,
            primary,
            locked: Vec::new(),
            kinds: Vec::new(),
            broken: Default::default(),
            broken_types: Vec::new(),
        }
    }

    #[test]
    fn a_company_is_recruitable_with_a_captain_unless_it_says_otherwise() {
        let unit = thing("[Thing]\n{\n\tids = pikeman\n\t[ElementComponent]\n}\n");
        let id = Id::from("pikeman");
        let things = HashMap::from([(&id, &unit)]);
        let layouts = HashMap::from([
            (Id::from("captained"), layout(Some(1))),
            (Id::from("leaderless"), layout(None)),
        ]);
        let company_of = |text: &str| company(&thing(text), &layouts, &things);
        let body = "\t[OrganizationComponent]\n\t{\n\t\tlayout_IDS = LAYOUT\n\
                    \t\t[Slot]\n\t\telement_IDS = pikeman\n\t\t[Slot]\n\t\telement_IDS = null\n\t}\n";
        let with = |layout: &str, extra: &str| {
            format!(
                "[Thing]\n{{\n\tids = c\n{extra}{}}}\n",
                body.replace("LAYOUT", layout)
            )
        };
        assert_eq!(company_of(&with("captained", "")), Some(vec![id.clone()]));
        assert_eq!(company_of(&with("leaderless", "")), None);
        assert_eq!(company_of(&with("captained", "\trecruitable = no\n")), None);
        let yes = with("leaderless", "\trecruitable = YES\n");
        assert_eq!(company_of(&yes), Some(vec![id]));
    }

    #[test]
    fn a_company_of_what_isnt_a_recruitable_element_isnt_recruitable() {
        let layouts = HashMap::from([(Id::from("l"), layout(Some(0)))]);
        let doodad = thing("[Thing]\n{\n\tids = doodad\n}\n");
        let hidden =
            thing("[Thing]\n{\n\tids = hidden\n\trecruitable = NO\n\t[ElementComponent]\n}\n");
        let (d, h) = (Id::from("doodad"), Id::from("hidden"));
        let things = HashMap::from([(&d, &doodad), (&h, &hidden)]);
        for unit in ["doodad", "hidden", "missing"] {
            let text = format!(
                "[Thing]\n{{\n\tids = c\n\t[OrganizationComponent]\n\t{{\n\t\tlayout_IDS = l\n\
                 \t\t[Slot]\n\t\telement_IDS = {unit}\n\t}}\n}}\n"
            );
            assert_eq!(company(&thing(&text), &layouts, &things), None, "{unit}");
        }
    }

    #[test]
    fn a_settlement_could_have_its_centres_its_structures_and_their_upgrades() {
        let settlement = thing(
            "[Thing]\n{\n\tids = s\n\t[SettlementComponent]\n\t{\n\
             \t\t[AvailableStructures]\n\t\t{\n\t\t\t[Structure]\n\t\t\tstructure_ids = woodmill\n\t\t}\n\
             \t\t[Levels]\n\t\t{\n\t\t\t[Level]\n\t\t\tsettlement_center_ids = village\n\t\t}\n\t}\n}\n",
        );
        let ids = |ids: &[&str]| ids.iter().map(|&i| Id::from(i)).collect::<Vec<_>>();
        let upgrades = HashMap::from([
            (Id::from("village"), ids(&["town"])),
            (Id::from("town"), ids(&["city"])),
            (Id::from("woodmill"), ids(&["sawmill", "woodexport"])),
        ]);
        let found =
            possible_structures(settlement.settlement_component.as_ref().unwrap(), &upgrades);
        let want: HashSet<Id> = ids(&[
            "village",
            "town",
            "city",
            "woodmill",
            "sawmill",
            "woodexport",
        ])
        .into_iter()
        .collect();
        assert_eq!(found, want);
        let needs = |groups: &[&[&str]]| Requires {
            structures: groups.iter().map(|g| ids(g)).collect(),
            any: false,
            properties: Vec::new(),
            hide: true,
        };
        assert!(buildable(None, &found));
        assert!(buildable(Some(&needs(&[&["sawmill"], &["town"]])), &found));
        assert!(buildable(Some(&needs(&[&["renamed", "city"]])), &found));
        assert!(!buildable(Some(&needs(&[&["town"], &["orc_pit"]])), &found));
    }
}
