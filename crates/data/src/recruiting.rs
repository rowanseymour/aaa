//! What recruiters list and what each listing needs, costs and takes to
//! arrive.

use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    GameData, Id, Layout, add_renames, defs, icon_beside, renamed, renames, resource_name,
    strings::localized, supply, ui_path,
};

/// How a template shows on the button that commissions or recruits it.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub name: String,
    /// Archive path of its button's icon.
    pub icon: Option<String>,
    /// What it costs, earns and takes to keep, by resource.
    pub cost: Vec<Amount>,
    pub production: Vec<Amount>,
    pub upkeep: Vec<Amount>,
}

/// So much of a resource.
#[derive(Debug, Clone, PartialEq)]
pub struct Amount {
    /// The resource's field name in the data, such as `money`.
    pub kind: &'static str,
    /// Its localized name, such as Money.
    pub resource: String,
    pub amount: f32,
}

/// The aircraft that flies a template to where it's recruited.
#[derive(Debug, Clone, PartialEq)]
pub struct Airlift {
    /// The aircraft's template.
    pub transport: Id,
    /// World units a second it flies at.
    pub speed: f32,
    /// The radius of the circle it turns in.
    pub turn_radius: f32,
    /// How far above the ground it flies.
    pub height: f32,
    /// Seconds between the things it drops.
    pub delay: f32,
}

/// How a template falls when it's dropped from the air.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fall {
    /// How far below what drops it it starts.
    pub height: f32,
    /// How much nearer it lands to where it's dropped than its fall alone
    /// would carry it.
    pub drag: f32,
}

/// How a template packs and unpacks.
#[derive(Debug, Clone, PartialEq)]
pub struct Deployment {
    /// For a packed building, such as a supply truck, the template it
    /// unpacks into.
    pub unpacks_into: Option<Id>,
    /// For a packed building, where it stands from the building it unpacks
    /// into, and the way it faces when the building packs into it, in world
    /// space whichever way the building faces.
    pub packed_offset: [f32; 2],
    pub packed_facing: [f32; 2],
    /// For a building, the seconds packing and unpacking take.
    pub packing: f32,
    pub unpacking: f32,
    /// For a building, the share of its health at or below which it can't
    /// pack.
    pub pack_health_min: f32,
}

impl Deployment {
    /// What a template's deployment leaves out, or one without a deployment.
    pub const DEFAULT: Deployment = Deployment {
        unpacks_into: None,
        packed_offset: [0.0, 0.0],
        packed_facing: [0.0, -1.0],
        packing: 7.0,
        unpacking: 7.0,
        pack_health_min: 0.5,
    };
}

impl Default for Deployment {
    fn default() -> Self {
        Deployment::DEFAULT
    }
}

/// What recruiting a template needs: buildings the player has standing,
/// and properties the recruiter or its side has.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Requires {
    /// Each building, by every id it may go by on a map: its current one
    /// first, then any it was renamed from.
    pub structures: Vec<Vec<Id>>,
    /// Whether one of them is enough, rather than all.
    pub any: bool,
    /// Every one of them, the recruiter's own or, for a technology, its
    /// side's.
    pub properties: Vec<Id>,
    /// Whether a recruiter missing one of `properties` leaves it out of
    /// its menu, rather than greying it.
    pub hide: bool,
}

/// A property a side can hold, as its research button shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Technology {
    pub name: String,
    pub description: Option<String>,
    /// Archive path of its button's icon.
    pub icon: Option<String>,
    /// What it says under its name: who it's for, then a line for each
    /// thing it does that the game has words for.
    pub lines: Vec<String>,
}

/// Where what a building recruits appears and goes, from where it stands:
/// x across its facing and y along it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeployPoints {
    pub creation: [f32; 2],
    pub deployment: [f32; 2],
}

impl GameData {
    /// What each template that recruits lists, in the order its buttons
    /// take, by template id: for a Corps HQ, what it commissions.
    pub fn recruit_lists(&self) -> HashMap<Id, Vec<Id>> {
        self.by_template(|t| {
            let list = &t.recruit_component.as_ref()?.recruit_actor;
            let ids: Vec<Id> = list.iter().filter_map(|r| r.actor_ids.clone()).collect();
            (!ids.is_empty()).then_some(ids)
        })
    }

    /// How each template that packs or unpacks does, by template id.
    pub fn deployments(&self) -> HashMap<Id, Deployment> {
        self.by_template(|t| {
            let m = t.mobile_deployment_component.as_ref()?;
            let d = Deployment::DEFAULT;
            Some(Deployment {
                unpacks_into: m
                    .unpack_to_actor_ids
                    .clone()
                    .filter(|i| *i != Id::from("null")),
                packed_offset: m.packed_position_offset.unwrap_or(d.packed_offset),
                packed_facing: m.packed_orientation.unwrap_or(d.packed_facing),
                packing: m.packing_length.unwrap_or(d.packing),
                unpacking: m.unpacking_length.unwrap_or(d.unpacking),
                pack_health_min: m.pack_health_min.unwrap_or(d.pack_health_min),
            })
        })
    }

    /// The packed building, such as a truck, that each building packs into,
    /// by template id: of those that unpack into it, the one defined first.
    pub fn packs_into(&self) -> HashMap<Id, Id> {
        let renames = self.thing_renames();
        let mut out = HashMap::new();
        for t in &self.definitions().things {
            let into = (t.thing.mobile_deployment_component.as_ref())
                .and_then(|m| m.unpack_to_actor_ids.clone())
                .filter(|i| *i != Id::from("null"));
            if let Some(into) = into {
                let into = renamed(&renames, &into);
                out.entry(into).or_insert_with(|| t.id.clone());
            }
        }
        add_renames(&mut out, &self.translations(), "Thing");
        out
    }

    /// Seconds recruiting or commissioning one of each template takes, by
    /// template id: an organization's recruit time, else the inverse of
    /// how fast it resupplies.
    pub fn recruit_times(&self) -> HashMap<Id, f32> {
        self.by_template(|t| match &t.organization_component {
            Some(o) => o.recruit_time,
            None => {
                let rate = t.element_component.as_ref()?.resupply_rate?;
                // The game takes half a tick for what never resupplies.
                Some(if rate > 0.0 { 1.0 / rate } else { 0.05 })
            }
        })
    }

    /// What buildings recruiting each template needs, by template id, for
    /// those that need any.
    pub fn requirements(&self) -> HashMap<Id, Requires> {
        let aliases = self.structure_aliases();
        self.by_template(|t| {
            let e = t.element_component.as_ref()?;
            let structures: Vec<Vec<Id>> = supply::listed(e.required_structures.as_deref())
                .iter()
                .map(&aliases)
                .collect();
            let properties = supply::listed(e.required_properties.as_deref());
            (!structures.is_empty() || !properties.is_empty()).then_some(Requires {
                structures,
                any: e.required_structures_any,
                properties,
                hide: t.hide_if_missing_properties.unwrap_or(true),
            })
        })
    }

    /// The properties a side can hold, by id.
    pub fn technologies(&self) -> Arc<HashMap<Id, Technology>> {
        self.kept("technologies", || self.read_technologies())
    }

    fn read_technologies(&self) -> HashMap<Id, Technology> {
        let strings = self.strings();
        let mut lines = self.property_lines(&strings);
        let definitions = self.definitions();
        (definitions.properties.iter())
            .filter(|(_, p)| p.is_technology)
            .map(|(id, p)| {
                let file = definitions
                    .property_files
                    .get(id)
                    .map_or("", String::as_str);
                let icon = icon_beside(file, p.action_button_icon.as_deref());
                let technology = Technology {
                    name: localized(&strings, p.name.as_deref()).unwrap_or_else(|| id.to_string()),
                    description: localized(&strings, p.description.as_deref()),
                    icon,
                    lines: lines.remove(id).unwrap_or_default(),
                };
                (id.clone(), technology)
            })
            .collect()
    }

    /// The transport each template that's carried to where it's recruited
    /// names, by template id.
    pub fn transports(&self) -> HashMap<Id, Id> {
        self.by_template(|t| (t.transport_actor.clone()).filter(|i| *i != Id::from("null")))
    }

    /// How each template that's flown to where it's recruited gets
    /// there, by template id.
    pub fn airlifts(&self) -> HashMap<Id, Airlift> {
        let things = &self.definitions().things;
        let plane = |id: &Id| {
            let t = &things.iter().find(|t| t.id == *id)?.thing;
            let mover = t.mover_component.as_ref()?;
            Some(Airlift {
                transport: id.clone(),
                speed: mover.speed_max?,
                turn_radius: mover.turn_radius.unwrap_or(0.0),
                height: t.aircraft_component.as_ref()?.operational_height?,
                delay: (t.transport_component.as_ref())
                    .and_then(|c| c.deployment_delay)
                    .unwrap_or(0.0),
            })
        };
        (self.transports().into_iter())
            .filter_map(|(cargo, transport)| Some((cargo, plane(&transport)?)))
            .collect()
    }

    /// How each template falls when it's dropped, by template id.
    pub fn falls(&self) -> HashMap<Id, Fall> {
        self.by_template(|t| {
            Some(Fall {
                height: t.size.iter().find_map(|s| s.height).unwrap_or(0.0),
                drag: t.fall_drag.unwrap_or(0.0),
            })
        })
    }

    /// How fast what falls gains speed, in world units a second each
    /// second.
    pub fn gravity(&self) -> Option<f32> {
        self.svars().iter().find_map(|v| v.gravity).map(f32::abs)
    }

    /// Every id a building a requirement names may go by on a map, as maps
    /// may still place one under any id it once had: its current one
    /// first, then those it was renamed from.
    pub(crate) fn structure_aliases(&self) -> impl Fn(&Id) -> Vec<Id> + use<> {
        let renames = self.thing_renames();
        let mut aliases: HashMap<Id, Vec<Id>> = HashMap::new();
        for (old, new) in &renames {
            aliases.entry(new.clone()).or_default().push(old.clone());
        }
        for olds in aliases.values_mut() {
            olds.sort_unstable();
        }
        move |s: &Id| {
            let id = renamed(&renames, s);
            let olds = aliases.get(&id).cloned().unwrap_or_default();
            std::iter::once(id).chain(olds).collect()
        }
    }

    /// Each renamed object template's old id to its new one.
    fn thing_renames(&self) -> HashMap<Id, Id> {
        renames(&self.translations(), "Thing")
    }

    /// Where what each template recruits appears, by template id.
    pub fn deploy_points(&self) -> HashMap<Id, DeployPoints> {
        self.by_template(|t| {
            let d = t.denizen_component.as_ref()?.deployment_info.as_ref()?;
            let creation = d.creation_offset.unwrap_or([0.0, 0.0]);
            Some(DeployPoints {
                creation,
                deployment: d.deployment_offset.unwrap_or(creation),
            })
        })
    }

    /// How each template that has a name shows on the button that
    /// commissions or recruits it, by template id. An organization costs
    /// what its captain, the one member it's raised with, does as well as
    /// its own, which for a Division HQ is all of it, and takes to keep its
    /// own upkeep and each member's.
    pub fn listings(&self) -> Arc<HashMap<Id, Listing>> {
        self.kept("listings", || self.read_listings())
    }

    fn read_listings(&self) -> HashMap<Id, Listing> {
        let strings = self.strings();
        let layouts = self.layouts();
        let definitions = self.definitions();
        let mut things: HashMap<&Id, &defs::Thing> = definitions
            .things
            .iter()
            .map(|t| (&t.id, &t.thing))
            .collect();
        // Templates may name their members by ids since renamed.
        let renames = self.thing_renames();
        let renamed: Vec<_> = (renames.iter())
            .map(|(old, new)| (old, things.get(new).copied()))
            .collect();
        for (old, thing) in renamed {
            match thing {
                Some(t) => things.insert(old, t),
                None => things.remove(old),
            };
        }
        self.by_template(|t| {
            let view = t.view.as_ref();
            let captain = t
                .organization_component
                .as_ref()
                .and_then(|o| o.slot.last()?.element_ids.as_ref())
                .and_then(|id| things.get(id));
            let economies: Vec<&defs::EconomyComponent> = [Some(t), captain.copied()]
                .into_iter()
                .flatten()
                .filter_map(|t| t.economy_component.as_ref())
                .collect();
            let economy = |of: fn(&defs::EconomyComponent) -> Option<&defs::Resources>| {
                amounts(&strings, economies.iter().filter_map(|&e| of(e)))
            };
            let members = members(t, &things, &layouts);
            Some(Listing {
                name: localized(&strings, t.name.as_deref())?,
                icon: view
                    .and_then(|v| v.icon.as_deref())
                    .filter(|i| i.to_ascii_lowercase().starts_with("/ui/"))
                    .and_then(ui_path),
                cost: economy(|e| e.cost.as_ref()),
                production: amounts(&strings, production(t, &members)),
                upkeep: amounts(&strings, upkeep(t, &members)),
            })
        })
    }
}

/// The resources given, totalled, with their localized names; those the
/// strings don't name keep their field name.
fn amounts<'a>(
    strings: &crate::Strings,
    of: impl Iterator<Item = &'a defs::Resources>,
) -> Vec<Amount> {
    let mut totals: Vec<(&str, f32)> = Vec::new();
    for (field, amount) in of.flat_map(defs::Resources::amounts) {
        match totals.iter_mut().find(|(f, _)| *f == field) {
            Some((_, total)) => *total += amount,
            None => totals.push((field, amount)),
        }
    }
    totals
        .into_iter()
        .filter(|&(_, amount)| amount != 0.0)
        .map(|(field, amount)| Amount {
            kind: field,
            resource: resource_name(strings, field),
            amount,
        })
        .collect()
}

/// An organization template's members when whole: each slot's, once for
/// every place its layout gives the slot in its starting formation, or
/// once where the layout isn't known.
fn members<'a>(
    t: &defs::Thing,
    things: &HashMap<&Id, &'a defs::Thing>,
    layouts: &HashMap<Id, Layout>,
) -> Vec<&'a defs::Thing> {
    let Some(o) = &t.organization_component else {
        return Vec::new();
    };
    let layout = o.layout_ids.as_ref().and_then(|l| layouts.get(l));
    let places = |slot: usize| {
        let start = layout.and_then(|l| l.formations.get(l.default));
        start.and_then(|f| f.slots.get(slot)).map_or(1, Vec::len)
    };
    (o.slot.iter().enumerate())
        .filter_map(|(i, s)| Some((i, *things.get(s.element_ids.as_ref()?)?)))
        .flat_map(|(i, member)| std::iter::repeat_n(member, places(i)))
        .collect()
}

/// What a template earns as it's shown: its own `[Production]`, or for a
/// regiment the sum of its `members`'.
fn production<'a>(
    t: &'a defs::Thing,
    members: &[&'a defs::Thing],
) -> impl Iterator<Item = &'a defs::Resources> {
    let earners = if members.is_empty() {
        vec![t]
    } else {
        members.to_vec()
    };
    earners
        .into_iter()
        .filter_map(|t| t.economy_component.as_ref()?.production.as_ref())
}

/// What a template takes to keep as it's shown: its own `[Upkeep]` and,
/// for a regiment, each of its `members`'.
fn upkeep<'a>(
    t: &'a defs::Thing,
    members: &[&'a defs::Thing],
) -> impl Iterator<Item = &'a defs::Resources> {
    std::iter::once(t)
        .chain(members.iter().copied())
        .filter_map(|t| t.economy_component.as_ref()?.upkeep.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Formation;

    #[test]
    fn production_is_the_templates_own_or_its_members() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\tIDS = truck\n\t[EconomyComponent]\n\t{\n\
             \t\t[Production]\n\t\t{\n\t\t\tmoney = 12\n\t\t\tunit_limit_provided = 5\n\t\t}\n\
             \t}\n}\n\
             [Thing]\n{\n\tIDS = regiment\n\t[EconomyComponent]\n\t{\n\
             \t\t[Production]\n\t\t{\n\t\t\tmoney = 99\n\t\t}\n\t}\n\
             \t[OrganizationComponent]\n\t{\n\
             \t\t[Slot]\n\t\t{\n\t\t\telement_ids = truck\n\t\t}\n\
             \t\t[Slot]\n\t\t{\n\t\t\telement_ids = truck\n\t\t}\n\t}\n}\n",
        );
        let things: Vec<(Id, defs::Thing)> = parsed
            .root
            .blocks_of("Thing")
            .zip(["truck", "regiment"])
            .map(|(b, id)| (Id::from(id), tgi::from_block(b).unwrap()))
            .collect();
        let by_id: HashMap<&Id, &defs::Thing> = things.iter().map(|(i, t)| (i, t)).collect();
        let earned = |id: &str| -> Vec<(&str, f32)> {
            let t = by_id[&Id::from(id)];
            let members = members(t, &by_id, &HashMap::new());
            amounts(&crate::Strings::default(), production(t, &members))
                .into_iter()
                .map(|a| (a.kind, a.amount))
                .collect()
        };
        assert_eq!(
            earned("truck"),
            [("money", 12.0), ("unit_limit_provided", 5.0)]
        );
        assert_eq!(
            earned("regiment"),
            [("money", 24.0), ("unit_limit_provided", 10.0)]
        );
    }

    #[test]
    fn upkeep_is_the_templates_own_and_each_members() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\tIDS = man\n\t[EconomyComponent]\n\t{\n\
             \t\t[Upkeep]\n\t\t{\n\t\t\tammo = 1\n\t\t}\n\t}\n}\n\
             [Thing]\n{\n\tIDS = regiment\n\t[EconomyComponent]\n\t{\n\
             \t\t[Upkeep]\n\t\t{\n\t\t\tunit_limit_consumed = 1\n\t\t}\n\t}\n\
             \t[OrganizationComponent]\n\t{\n\t\tlayout_ids = line\n\
             \t\t[Slot]\n\t\t{\n\t\t\telement_ids = man\n\t\t}\n\
             \t\t[Slot]\n\t\t{\n\t\t\telement_ids = man\n\t\t}\n\
             \t\t[Slot]\n\t\t{\n\t\t\telement_ids = man\n\t\t}\n\t}\n}\n",
        );
        let things: Vec<(Id, defs::Thing)> = parsed
            .root
            .blocks_of("Thing")
            .zip(["man", "regiment"])
            .map(|(b, id)| (Id::from(id), tgi::from_block(b).unwrap()))
            .collect();
        let by_id: HashMap<&Id, &defs::Thing> = things.iter().map(|(i, t)| (i, t)).collect();
        let regiment = by_id[&Id::from("regiment")];
        let kept = |layouts: &HashMap<Id, Layout>| -> Vec<(&str, f32)> {
            let members = members(regiment, &by_id, layouts);
            amounts(&crate::Strings::default(), upkeep(regiment, &members))
                .into_iter()
                .map(|a| (a.kind, a.amount))
                .collect()
        };
        let whole = [("unit_limit_consumed", 1.0), ("ammo", 3.0)];
        assert_eq!(kept(&HashMap::new()), whole);
        // A slot's member counts for each place its layout gives the slot.
        let line = Layout {
            formations: vec![Formation {
                id: Id::from("column"),
                slots: vec![vec![[0.0; 2]; 3], vec![[0.0; 2]], vec![[0.0; 2]]],
                aggressive: false,
            }],
            default: 0,
            combat: None,
            primary: None,
        };
        assert_eq!(
            kept(&HashMap::from([(Id::from("line"), line)])),
            [("unit_limit_consumed", 1.0), ("ammo", 5.0)]
        );
    }
}
