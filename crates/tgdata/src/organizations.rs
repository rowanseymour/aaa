//! Organizations: regiments and herds, the layouts their members stand
//! in, and how far they reach.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use crate::{Efficiency, GameData, Id, defs, scoped};

/// How far around it an organization reaches.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ranges {
    /// How far from where it stands it takes on enemies that fire on it.
    pub operational: Option<f32>,
    /// Its zone of control's radius.
    pub control: Option<f32>,
}

/// What an organization template, such as a regiment, is made of: its
/// organization component.
#[derive(Debug, Clone, PartialEq)]
pub struct OrganizationComponent {
    /// The [`Layout`] its members are arranged by.
    pub layout: Id,
    /// The template filling each of the layout's slots; `None` for one left
    /// empty.
    pub slots: Vec<Option<Id>>,
}

/// How an organization's members can be arranged.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The formations it can take, in the order the layout lists them.
    pub formations: Vec<Formation>,
    /// Index into `formations` of the one it starts in.
    pub default: usize,
    /// Index into `formations` of the one it fights in once engaged.
    pub combat: Option<usize>,
    /// The slot whose member the organization's position follows.
    pub primary: Option<usize>,
    /// What its broken formation, while it has none, sets its attack
    /// efficiency to.
    pub broken: Efficiency,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Formation {
    /// E.g. `regiment_defensive`.
    pub id: Id,
    /// The positions of each slot's members, as many as the slot holds:
    /// offsets from the primary slot's position, with 0,1 straight ahead.
    pub slots: Vec<Vec<[f32; 2]>>,
    /// Whether a regiment standing or moving in it takes on enemies that
    /// come within its operational range, or runs into.
    pub aggressive: bool,
    /// What it sets its regiment's attack efficiency to.
    pub efficiency: Efficiency,
}

impl GameData {
    /// Organizations' ranges, by template id, including the ids the game
    /// has since renamed.
    pub fn organization_ranges(&self) -> HashMap<Id, Ranges> {
        self.by_template(|t| {
            let o = t.organization_component.as_ref()?;
            Some(Ranges {
                operational: o.operational_range,
                control: o.control_range,
            })
        })
    }

    /// What every organization template is made of, by template id,
    /// including the ids the game has since renamed.
    pub fn organization_components(&self) -> HashMap<Id, OrganizationComponent> {
        self.by_template(|t| {
            let o = t.organization_component.as_ref()?;
            Some(OrganizationComponent {
                layout: o.layout_ids.clone()?,
                // Templates name an empty slot's element `null`.
                slots: o
                    .slot
                    .iter()
                    .map(|s| s.element_ids.clone().filter(|e| *e != Id::from("null")))
                    .collect(),
            })
            .filter(|o| !o.slots.is_empty())
        })
    }

    /// The organization layouts, by id.
    pub fn layouts(&self) -> Arc<HashMap<Id, Layout>> {
        self.kept("layouts", || self.read_layouts())
    }

    fn read_layouts(&self) -> HashMap<Id, Layout> {
        let files =
            self.parse_tgis(|n| n.starts_with("organizations/") || n.starts_with("templates/"));
        let files = scoped(&files);
        let aggressive: HashSet<Id> = files
            .iter()
            .flat_map(|f| f.read::<defs::Formation>("Formation"))
            .filter(|f| f.aggressive)
            .filter_map(|f| f.ids)
            .collect();
        let mut efficiencies = HashMap::new();
        for scope in &files {
            for block in scope.root.blocks_of("Formation") {
                let ids = scope
                    .deserialize::<defs::Formation>(block)
                    .and_then(|f| f.ids);
                let held = scope.deserialize::<defs::FormationProperties>(block);
                let held = held.map_or_else(Vec::new, |h| h.property);
                let efficiency =
                    (held.iter().map(Efficiency::of)).fold(Efficiency::default(), Efficiency::with);
                // The first of those defined more than once, as for other
                // properties.
                if let Some(ids) = ids {
                    efficiencies.entry(ids).or_insert(efficiency);
                }
            }
        }
        let known = Known {
            aggressive: &aggressive,
            efficiencies: &efficiencies,
        };
        files
            .iter()
            .flat_map(|f| f.read::<defs::OrganizationLayout>("OrganizationLayout"))
            .filter_map(|l| Some((l.ids.clone()?, layout(&l, known)?)))
            .collect()
    }

    /// What each organization template's broken formation sets its attack
    /// efficiency to, by template id, for those whose layout has one.
    pub fn broken_efficiencies(&self) -> HashMap<Id, Efficiency> {
        let layouts = self.layouts();
        (self.organization_components().into_iter())
            .filter_map(|(t, o)| Some((t, layouts.get(&o.layout)?.broken)))
            .filter(|(_, e)| e.value().is_some())
            .collect()
    }

    /// What each formation sets its regiment's attack efficiency to, by
    /// formation id, for those that set it.
    pub fn formation_efficiencies(&self) -> HashMap<Id, Efficiency> {
        let layouts = self.layouts();
        (layouts.values().flat_map(|l| &l.formations))
            .filter(|f| f.efficiency.value().is_some())
            .map(|f| (f.id.clone(), f.efficiency))
            .collect()
    }
}

/// What the formations themselves say, by formation id.
#[derive(Clone, Copy)]
struct Known<'a> {
    aggressive: &'a HashSet<Id>,
    efficiencies: &'a HashMap<Id, Efficiency>,
}

/// A layout's formations, each with its positions given to the slots in
/// turn: a slot takes the next position for its property.
fn layout(l: &defs::OrganizationLayout, known: Known) -> Option<Layout> {
    let formations: Vec<Formation> = l
        .formation
        .iter()
        .filter_map(|f| {
            let id = f.formation_ids.clone()?;
            Some(Formation {
                aggressive: known.aggressive.contains(&id),
                efficiency: known.efficiencies.get(&id).copied().unwrap_or_default(),
                slots: slot_positions(&l.slot, f)?,
                id,
            })
        })
        .collect();
    let index = |id: &Option<Id>| {
        let id = id.as_ref()?;
        formations.iter().position(|f| &f.id == id)
    };
    // Layouts may inherit a default they don't define.
    let default = index(&l.default_formation_ids).unwrap_or(0);
    let combat = index(&l.combat_formation_ids);
    (!formations.is_empty()).then(|| Layout {
        formations,
        default,
        combat,
        primary: l.slot.iter().position(|s| s.primary),
        broken: l
            .broken_property
            .as_ref()
            .map(Efficiency::of)
            .unwrap_or_default(),
    })
}

fn slot_positions(
    slots: &[defs::LayoutSlot],
    formation: &defs::LayoutFormation,
) -> Option<Vec<Vec<[f32; 2]>>> {
    let mut taken = vec![false; formation.position.len()];
    slots
        .iter()
        .map(|slot| {
            let property = slot.property_ids.as_deref()?;
            (0..slot.number.unwrap_or(1))
                .map(|_| {
                    let i = formation.position.iter().enumerate().position(|(i, p)| {
                        !taken[i]
                            && p.property_ids
                                .as_deref()
                                .is_some_and(|q| q.eq_ignore_ascii_case(property))
                    })?;
                    taken[i] = true;
                    Some(formation.position[i].offset.unwrap_or([0.0, 0.0]))
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_give_positions_to_slots_by_property() {
        let parsed = tgi::parse(
            "[OrganizationLayout]\n{\n\tIDS = squad\n\tdefault_formation_ids = missing\n\
             \tcombat_formation_ids = line\n\
             \t[BrokenProperty]\n\t{\n\t\t[Modifier]\n\t\t{\n\t\t\ttype = MULTIPLY\n\
             \t\t\tattribute = ATTACK_EFFICIENCY_DESIRED\n\t\t\tamount = -.5\n\t\t}\n\t}\n\
             \t[Slot]\n\tproperty_IDS = front\n\tnumber = 2\n\
             \t[Slot]\n\tproperty_IDS = captain\n\tprimary = true\n\
             \t[Formation]\n\t{\n\t\tformation_IDS = line\n\
             \t\t[Position]\n\t\tproperty_IDS = captain\n\t\toffset = 0,0\n\
             \t\t[Position]\n\t\tproperty_IDS = front\n\t\toffset = -1,1\n\
             \t\t[Position]\n\t\tproperty_IDS = front\n\t\toffset = 1,1\n\t}\n}\n",
        );
        let block = parsed.root.blocks_of("OrganizationLayout").next().unwrap();
        let l: defs::OrganizationLayout = tgi::from_block(block).unwrap();
        let efficiencies = [(
            Id::from("line"),
            Efficiency {
                actual: None,
                desired: Some(0.75),
            },
        )];
        let efficiencies = efficiencies.into();
        let known = Known {
            aggressive: &HashSet::new(),
            efficiencies: &efficiencies,
        };
        let layout = layout(&l, known).unwrap();
        assert_eq!(layout.broken.desired, Some(0.5));
        assert_eq!(layout.formations[0].efficiency.desired, Some(0.75));
        assert_eq!(layout.default, 0);
        assert_eq!(layout.combat, Some(0));
        assert_eq!(layout.primary, Some(1));
        assert_eq!(layout.formations[0].id, Id::from("line"));
        assert_eq!(
            layout.formations[0].slots,
            [vec![[-1.0, 1.0], [1.0, 1.0]], vec![[0.0, 0.0]]]
        );
    }
}
