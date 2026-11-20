//! Organizations: regiments and herds, the layouts their members stand
//! in, and how far they reach.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use crate::{CombatEffect, GameData, Id, defs, scoped};

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
    /// Each slot's property, e.g. `front`, which the object display groups
    /// members' cards by.
    pub kinds: Vec<Id>,
    /// What its broken formation's property gives it while it has no
    /// formation.
    pub broken: CombatEffect,
    /// The target types its broken formation's property gives.
    pub broken_types: Vec<Id>,
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
    /// What its properties give the regiment in it.
    pub effect: CombatEffect,
    /// The target types its properties give.
    pub types: Vec<Id>,
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
        let mut effects = HashMap::new();
        for scope in &files {
            for block in scope.root.blocks_of("Formation") {
                let ids = scope
                    .deserialize::<defs::Formation>(block)
                    .and_then(|f| f.ids);
                let held = scope.deserialize::<defs::FormationProperties>(block);
                let held = held.map_or_else(Vec::new, |h| h.property);
                let effect = (held.iter().filter_map(crate::experience::combat_effect))
                    .fold(CombatEffect::default(), |a, b| a.with(&b));
                // The first of those defined more than once, as for other
                // properties.
                if let Some(ids) = ids {
                    let types = held.iter().flat_map(types_of).collect();
                    effects.entry(ids).or_insert(Gives { effect, types });
                }
            }
        }
        let known = Known {
            aggressive: &aggressive,
            effects: &effects,
        };
        files
            .iter()
            .flat_map(|f| f.read::<defs::OrganizationLayout>("OrganizationLayout"))
            .filter_map(|l| Some((l.ids.clone()?, layout(&l, known)?)))
            .collect()
    }

    /// What each organization template's broken formation's property
    /// gives it, by template id, for those whose layout has one.
    pub fn broken_effects(&self) -> HashMap<Id, CombatEffect> {
        let layouts = self.layouts();
        (self.organization_components().into_iter())
            .filter_map(|(t, o)| Some((t, layouts.get(&o.layout)?.broken.clone())))
            .filter(|(_, e)| *e != CombatEffect::default())
            .collect()
    }

    /// The target types each organization template's broken formation's
    /// property gives it while it has no formation, by template id, for
    /// those that give any.
    pub fn broken_types(&self) -> HashMap<Id, Vec<Id>> {
        let layouts = self.layouts();
        (self.organization_components().into_iter())
            .filter_map(|(t, o)| Some((t, layouts.get(&o.layout)?.broken_types.clone())))
            .filter(|(_, types)| !types.is_empty())
            .collect()
    }

    /// The target types each formation's properties give the regiment in
    /// it, by formation id, for those that give any.
    pub fn formation_types(&self) -> HashMap<Id, Vec<Id>> {
        let layouts = self.layouts();
        (layouts.values().flat_map(|l| &l.formations))
            .filter(|f| !f.types.is_empty())
            .map(|f| (f.id.clone(), f.types.clone()))
            .collect()
    }

    /// Where the regiments of a group all in one formation stand, from
    /// the group's centre in the group's order, by that formation's id,
    /// for those that name a group formation.
    pub fn group_formations(&self) -> HashMap<Id, Vec<[f32; 2]>> {
        let files = self.parse_tgis(|n| n.starts_with("organizations/") || n.starts_with("game/"));
        let files = scoped(&files);
        let groups: HashMap<Id, Vec<[f32; 2]>> = (files.iter())
            .flat_map(|f| f.read::<defs::GroupFormation>("GroupFormation"))
            .filter_map(|g| Some((g.ids?, g.position.iter().filter_map(|p| p.offset).collect())))
            .collect();
        (files.iter())
            .flat_map(|f| f.read::<defs::Formation>("Formation"))
            .filter_map(|f| Some((f.ids?, groups.get(&f.group_formation_ids?)?.clone())))
            .collect()
    }

    /// What each formation's properties give the regiment in it, by
    /// formation id, for those that give anything.
    pub fn formation_effects(&self) -> HashMap<Id, CombatEffect> {
        let layouts = self.layouts();
        (layouts.values().flat_map(|l| &l.formations))
            .filter(|f| f.effect != CombatEffect::default())
            .map(|f| (f.id.clone(), f.effect.clone()))
            .collect()
    }
}

/// What the formations themselves say, by formation id.
#[derive(Clone, Copy)]
struct Known<'a> {
    aggressive: &'a HashSet<Id>,
    effects: &'a HashMap<Id, Gives>,
}

/// What a formation's properties give the regiment in it.
#[derive(Clone, Default)]
struct Gives {
    effect: CombatEffect,
    types: Vec<Id>,
}

/// The target types a property gives what holds it.
fn types_of(p: &defs::Property) -> Vec<Id> {
    crate::supply::listed(p.enabled_target_flags.as_deref())
}

/// A layout's formations, each with its positions given to the slots in
/// turn: a slot takes the next position for its property.
fn layout(l: &defs::OrganizationLayout, known: Known) -> Option<Layout> {
    let formations: Vec<Formation> = l
        .formation
        .iter()
        .filter_map(|f| {
            let id = f.formation_ids.clone()?;
            let gives = known.effects.get(&id).cloned().unwrap_or_default();
            Some(Formation {
                aggressive: known.aggressive.contains(&id),
                effect: gives.effect,
                types: gives.types,
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
        kinds: (l.slot.iter())
            .map(|s| Id::from(s.property_ids.as_deref().unwrap_or_default()))
            .collect(),
        broken: (l.broken_property.as_ref())
            .and_then(crate::experience::combat_effect)
            .unwrap_or_default(),
        broken_types: l.broken_property.as_ref().map(types_of).unwrap_or_default(),
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
    use crate::Efficiency;

    #[test]
    fn layouts_give_positions_to_slots_by_property() {
        let parsed = tgi::parse(
            "[OrganizationLayout]\n{\n\tIDS = squad\n\tdefault_formation_ids = missing\n\
             \tcombat_formation_ids = line\n\
             \t[BrokenProperty]\n\t{\n\t\t[Modifier]\n\t\t{\n\t\t\ttype = MULTIPLY\n\
             \t\t\tattribute = ATTACK_EFFICIENCY_DESIRED\n\t\t\tamount = -.5\n\t\t}\n\
             \t\t[Modifier]\n\t\t{\n\t\t\ttype = MULTIPLY\n\
             \t\t\tattribute = SPEED_MAX\n\t\t\tamount = .25\n\t\t}\n\
             \t\tenabled_target_flags = scattered\n\t}\n\
             \t[Slot]\n\tproperty_IDS = front\n\tnumber = 2\n\
             \t[Slot]\n\tproperty_IDS = captain\n\tprimary = true\n\
             \t[Formation]\n\t{\n\t\tformation_IDS = line\n\
             \t\t[Position]\n\t\tproperty_IDS = captain\n\t\toffset = 0,0\n\
             \t\t[Position]\n\t\tproperty_IDS = front\n\t\toffset = -1,1\n\
             \t\t[Position]\n\t\tproperty_IDS = front\n\t\toffset = 1,1\n\t}\n}\n",
        );
        let block = parsed.root.blocks_of("OrganizationLayout").next().unwrap();
        let l: defs::OrganizationLayout = tgi::from_block(block).unwrap();
        let line = CombatEffect {
            speed: -0.25,
            efficiency: Efficiency {
                actual: None,
                desired: Some(0.75),
            },
            ..Default::default()
        };
        let gives = Gives {
            effect: line,
            types: vec![Id::from("drilled")],
        };
        let effects = [(Id::from("line"), gives)].into();
        let known = Known {
            aggressive: &HashSet::new(),
            effects: &effects,
        };
        let layout = layout(&l, known).unwrap();
        assert_eq!(layout.broken.efficiency.desired, Some(0.5));
        assert_eq!(layout.broken.speed, 0.25);
        assert_eq!(layout.formations[0].effect.efficiency.desired, Some(0.75));
        assert_eq!(layout.formations[0].effect.speed, -0.25);
        assert_eq!(layout.broken_types, [Id::from("scattered")]);
        assert_eq!(layout.kinds, [Id::from("front"), Id::from("captain")]);
        assert_eq!(layout.formations[0].types, [Id::from("drilled")]);
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
