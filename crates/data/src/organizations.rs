//! Organizations: regiments and herds, the layouts their members stand
//! in, and how far they reach.

use std::collections::HashMap;
use std::collections::HashSet;

use crate::{GameData, Id, defs, scoped};

/// How far around it an organization reaches.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ranges {
    /// How far from where it stands it takes on enemies that fire on it.
    pub operational: Option<f32>,
    /// Its zone of control's radius.
    pub control: Option<f32>,
}

/// What an organization template, such as a regiment, is made of.
#[derive(Debug, Clone, PartialEq)]
pub struct Organization {
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
}

impl GameData {
    /// Organizations' ranges, by template id, including the ids the game
    /// has since renamed.
    pub fn organization_ranges(&mut self) -> HashMap<Id, Ranges> {
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
    pub fn organizations(&mut self) -> HashMap<Id, Organization> {
        self.by_template(|t| {
            let o = t.organization_component.as_ref()?;
            Some(Organization {
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
    pub fn layouts(&mut self) -> HashMap<Id, Layout> {
        let files =
            self.parse_tgis(|n| n.starts_with("organizations/") || n.starts_with("templates/"));
        let files = scoped(&files);
        let aggressive: HashSet<Id> = files
            .iter()
            .flat_map(|f| f.read::<defs::Formation>("Formation"))
            .filter(|f| f.aggressive)
            .filter_map(|f| f.ids)
            .collect();
        files
            .iter()
            .flat_map(|f| f.read::<defs::OrganizationLayout>("OrganizationLayout"))
            .filter_map(|l| Some((l.ids.clone()?, layout(&l, &aggressive)?)))
            .collect()
    }
}

/// A layout's formations, each with its positions given to the slots in
/// turn: a slot takes the next position for its property.
fn layout(l: &defs::OrganizationLayout, aggressive: &HashSet<Id>) -> Option<Layout> {
    let formations: Vec<Formation> = l
        .formation
        .iter()
        .filter_map(|f| {
            let id = f.formation_ids.clone()?;
            Some(Formation {
                aggressive: aggressive.contains(&id),
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
             \t[Slot]\n\tproperty_IDS = front\n\tnumber = 2\n\
             \t[Slot]\n\tproperty_IDS = captain\n\tprimary = true\n\
             \t[Formation]\n\t{\n\t\tformation_IDS = line\n\
             \t\t[Position]\n\t\tproperty_IDS = captain\n\t\toffset = 0,0\n\
             \t\t[Position]\n\t\tproperty_IDS = front\n\t\toffset = -1,1\n\
             \t\t[Position]\n\t\tproperty_IDS = front\n\t\toffset = 1,1\n\t}\n}\n",
        );
        let block = parsed.root.blocks_of("OrganizationLayout").next().unwrap();
        let l: defs::OrganizationLayout = tgi::from_block(block).unwrap();
        let layout = layout(&l, &HashSet::new()).unwrap();
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
