//! What bears on supply beyond who gives it: the settings it runs by, what
//! properties do to it, and the special operations that give it.

use std::collections::HashMap;

use crate::{GameData, Id, Shape, Targets, defs, soft_shape};

/// The settings supply runs by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SupplyRules {
    /// Seconds a connected relay's stockpile gains each second.
    pub recharge: f32,
    /// Seconds a bombardment's hit keeps a regiment out of supply.
    pub bombard_timeout: f32,
    /// The least a multiplier that modifiers add up to comes to.
    pub multiply_floor: f32,
    /// Morale a second a regiment regaining it gains more in supply.
    pub morale_bonus: f32,
}

impl Default for SupplyRules {
    fn default() -> Self {
        SupplyRules {
            recharge: 1.0,
            bombard_timeout: 1.0,
            multiply_floor: 0.05,
            morale_bonus: 0.5,
        }
    }
}

/// What a property does to the supply of what has it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SupplyEffect {
    /// Added to the range of a provider.
    pub range: f32,
    /// Added to the 1 a regiment's resupply rates are multiplied by.
    pub resupply: f32,
    /// A regiment is in supply with no HQ and, while its side can pay its
    /// upkeep, outside every zone.
    pub supplied: bool,
    /// For a technology, the properties a thing of the side needs for it
    /// to be on it.
    pub requires: Vec<Id>,
}

/// What a template's things are, for what works only on some.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Kind {
    /// The shared properties it has, such as `org_unit`.
    pub properties: Vec<Id>,
    /// The flags its properties give it for [`Targets`] to match.
    pub flags: Vec<Id>,
    /// Its soft shape: what's aimed at a point reaches as far as the
    /// circle round it.
    pub soft: Shape,
}

/// A special operation that bears on supply, as far as it does.
#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    /// Gives regiments around where it's aimed a property for a time.
    Enchant {
        targets: Targets,
        /// How far from where it's aimed it reaches, to the edge of a
        /// regiment's soft shape.
        area: f32,
        /// The most regiments it works on, the nearest; 0 for all.
        max_targets: u32,
        property: Id,
        seconds: f32,
    },
    /// Drops something of this template that gives supply where it's
    /// aimed.
    Drop(Id),
}

/// The ids a comma separated list names.
pub(crate) fn listed(list: Option<&str>) -> Vec<Id> {
    let ids = list.unwrap_or("").split(',').map(str::trim);
    ids.filter(|id| !id.is_empty() && !id.eq_ignore_ascii_case("null"))
        .map(Id::from)
        .collect()
}

/// Whether a modifier's field names `what`.
pub(crate) fn is(name: &Option<String>, what: &str) -> bool {
    name.as_deref()
        .is_some_and(|n| n.eq_ignore_ascii_case(what))
}

fn effect(p: &defs::Property) -> Option<SupplyEffect> {
    let mut out = SupplyEffect {
        requires: listed(p.required_properties.as_deref()),
        ..SupplyEffect::default()
    };
    for m in &p.modifier {
        let amount = m.amount.unwrap_or(0.0);
        if is(&m.attribute, "SUPPLY_RANGE") {
            out.range += amount;
        }
        if is(&m.attribute, "RESUPPLY_RATE") {
            out.resupply += amount;
        }
        out.supplied |= is(&m.modifier_flag, "SUPPLIED");
    }
    (out.range != 0.0 || out.resupply != 0.0 || out.supplied).then_some(out)
}

/// The properties a template's abilities give what they land on.
fn enchantments(t: &defs::Thing) -> impl Iterator<Item = &defs::Property> {
    let performs = t.performer_component.iter().flat_map(|p| &p.ability);
    let defends = t.defender_component.iter().flat_map(|d| &d.ability);
    (performs.chain(defends))
        .filter_map(|a| a.ability_definition.as_ref())
        .flat_map(|d| &d.effect)
        .filter_map(|e| e.enchantment_property.as_ref())
}

impl GameData {
    pub fn supply_rules(&mut self) -> SupplyRules {
        let svars = self.svars();
        let default = SupplyRules::default();
        let of = |get: fn(&defs::SVars) -> Option<f32>, or: f32| {
            svars.iter().find_map(get).unwrap_or(or)
        };
        SupplyRules {
            recharge: of(|v| v.supply_stockpile_recharge_rate, default.recharge),
            bombard_timeout: of(|v| v.bombard_target_timeout, default.bombard_timeout),
            multiply_floor: of(
                |v| v.modifier_attribute_clamp_multiply,
                default.multiply_floor,
            ),
            morale_bonus: of(|v| v.morale_supply_bonus, default.morale_bonus),
        }
    }

    /// What each property does to supply, for those that do anything to
    /// it, by its id: the shared ones, technologies among them, and those
    /// abilities give.
    pub fn supply_effects(&mut self) -> HashMap<Id, SupplyEffect> {
        let definitions = self.definitions();
        let shared = definitions.properties.values();
        let given = definitions
            .things
            .iter()
            .flat_map(|t| enchantments(&t.thing));
        let operations = (definitions.abilities.iter())
            .flat_map(|(_, a)| &a.effect)
            .filter_map(|e| e.enchantment_property.as_ref());
        // The first of those given more than once, in the order they load,
        // so every copy of a match reads the same.
        let mut out = HashMap::new();
        for p in shared.chain(given).chain(operations) {
            if let (Some(id), Some(effect)) = (p.ids.clone(), effect(p)) {
                out.entry(id).or_insert(effect);
            }
        }
        out
    }

    /// Each template's [`Kind`], for those with properties, by template id.
    pub fn kinds(&mut self) -> HashMap<Id, Kind> {
        let definitions = self.definitions();
        self.by_template(|t| {
            let c = t.properties_component.as_ref()?;
            let properties: Vec<Id> =
                (c.property.iter().filter_map(|r| r.property_ids.clone())).collect();
            let shared = properties
                .iter()
                .filter_map(|p| definitions.properties.get(p));
            let flags = shared
                .chain(&c.intrinsic_property)
                .flat_map(|p| listed(p.enabled_target_flags.as_deref()))
                .collect();
            Some(Kind {
                properties,
                flags,
                soft: soft_shape(t),
            })
        })
    }

    /// The health each template's things lose a second to their own
    /// properties, for those that lose any, by template id.
    pub fn decays(&mut self) -> HashMap<Id, f32> {
        let definitions = self.definitions();
        self.by_template(|t| {
            let c = t.properties_component.as_ref()?;
            let shared = c.property.iter().filter_map(|r| r.property_ids.as_ref());
            let shared = shared.filter_map(|p| definitions.properties.get(p));
            let periodic = |m: &&defs::Modifier| {
                (m.kind.as_deref()).is_some_and(|k| k.eq_ignore_ascii_case("PERIODIC_DAMAGE"))
            };
            let modifiers = shared
                .chain(&c.intrinsic_property)
                .flat_map(|p| &p.modifier);
            let rate: f32 = modifiers.filter(periodic).filter_map(|m| m.amount).sum();
            (rate > 0.0).then_some(rate)
        })
    }

    /// The special operations that bear on supply, by id.
    pub fn operations(&mut self) -> HashMap<Id, Operation> {
        let definitions = self.definitions();
        let effects = self.supply_effects();
        let supplies = self.supplies();
        let mut out = HashMap::new();
        for (id, a) in &definitions.abilities {
            let gives = a.effect.iter().find_map(|e| {
                let p = e.enchantment_property.as_ref()?;
                let property = p.ids.clone().filter(|id| effects.contains_key(id))?;
                Some((property, p.duration.unwrap_or(0.0)))
            });
            let objects = a.effect.iter().filter_map(|e| e.object_ids.as_ref());
            let drops = objects.clone().find(|&o| supplies.contains_key(o)).cloned();
            let operation = match (gives, drops) {
                (Some((property, seconds)), _) => Operation::Enchant {
                    targets: Targets::parse(a.targets.as_deref().unwrap_or("")),
                    area: a.area,
                    max_targets: a.max_targets.unwrap_or(0),
                    property,
                    seconds,
                },
                (None, Some(template)) => Operation::Drop(template),
                (None, None) => continue,
            };
            out.insert(id.clone(), operation);
        }
        out
    }
}
