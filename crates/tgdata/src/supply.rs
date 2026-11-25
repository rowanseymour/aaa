//! What bears on supply beyond who gives it: the settings it runs by, what
//! properties do to it, and the special operations that give it.

use std::collections::{HashMap, HashSet};

use crate::combat::soft_shape;
use crate::strikes::strike;
use crate::{Damage, GameData, Id, Shape, Strike, Targets, defs};

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

/// What a special operation does in play, for those the match carries
/// out.
#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    /// Gives what's around where it's aimed a property for a time.
    Enchant(Enchantment),
    /// Drops something of this template that gives supply where it's
    /// aimed.
    Drop(Id),
    /// Lifts the fog from its user's minimap for a time.
    Reveal { seconds: f32 },
    /// Does damage round where it's aimed, once it lands.
    Strike(Box<Strike>),
    /// Brings something new onto the map for its user.
    Summon(Summon),
}

/// What an operation that gives a property for a time does.
#[derive(Debug, Clone, PartialEq)]
pub struct Enchantment {
    /// What it works on: regiments, or units and buildings one by one.
    pub targets: Targets,
    /// How far from where it's aimed it reaches, to the edge of what it
    /// works on.
    pub area: f32,
    /// The most things it works on, the nearest; 0 for all.
    pub max_targets: u32,
    pub property: Id,
    pub seconds: f32,
    /// The property's enchantment type, which immunities name.
    pub kind: Option<Id>,
    /// Morale each thing it works on gives its regiment; negative for a
    /// loss.
    pub morale: f32,
    /// Main damage it does first to those of what it works on that its
    /// own targets match.
    pub damage: Vec<(Targets, Damage)>,
    /// Seconds after it's used that it takes effect.
    pub delay: f32,
}

/// What a summoning operation brings, and what holds it back.
#[derive(Debug, Clone, PartialEq)]
pub struct Summon {
    /// The template of what it brings.
    pub template: Id,
    /// It's aimed only on the map's main land.
    pub main_land: bool,
    /// It isn't ready while anything it put down is still there.
    pub one_at_a_time: bool,
    /// It isn't ready while its user owns anything with this flag.
    pub unique: Option<Id>,
}

/// How long an operation lifts the fog from its user's minimap, for one
/// that gives a property doing that: the property as the operation gives
/// it, else as it's defined on its own.
pub(crate) fn reveals(
    a: &defs::AbilityDefinition,
    properties: &HashMap<Id, defs::Property>,
) -> Option<f32> {
    a.effect.iter().find_map(|e| {
        let p = e.enchantment_property.as_ref()?;
        let shared = p.ids.as_ref().and_then(|id| properties.get(id));
        let modifiers = match p.modifier.is_empty() {
            true => shared.map_or(&p.modifier, |s| &s.modifier),
            false => &p.modifier,
        };
        let flag = |m: &defs::Modifier| is(&m.modifier_flag, "MINIMAP_NO_FOG");
        let seconds = p.duration.or_else(|| shared?.duration).unwrap_or(0.0);
        modifiers.iter().any(flag).then_some(seconds)
    })
}

/// What an operation that gives a property for a time does, for one
/// that does.
fn enchantment(
    a: &defs::AbilityDefinition,
    flights: &HashMap<Id, crate::Flight>,
) -> Option<Enchantment> {
    let p = a
        .effect
        .iter()
        .find_map(|e| e.enchantment_property.as_ref())?;
    let amount = |kind: &str| {
        let of = a.effect.iter().filter(|e| is(&e.kind, kind));
        of.filter_map(|e| e.amount).sum::<f32>()
    };
    let damage = (a.effect.iter())
        .filter(|e| is(&e.kind, "MAIN_DAMAGE"))
        .filter_map(|e| {
            let targets = Targets::of(e.targets.as_deref());
            let kind = e.damage_type_ids.clone()?;
            Some((
                targets,
                Damage {
                    kind,
                    amount: e.amount?,
                },
            ))
        })
        .collect();
    let projectile = a.projectile_ids.as_ref().and_then(|p| flights.get(p));
    Some(Enchantment {
        targets: Targets::of(a.targets.as_deref()),
        area: a.area,
        max_targets: a.max_targets.unwrap_or(0),
        property: p.ids.clone()?,
        seconds: p.duration.unwrap_or(0.0),
        kind: p.enchantment_type.clone(),
        morale: amount("MORALE_GAIN") - amount("MORALE_LOSS"),
        damage,
        delay: projectile.map_or(0.0, |f| f.life),
    })
}

/// The ids a comma separated list names.
pub(crate) fn listed(list: Option<&str>) -> Vec<Id> {
    let ids = list.unwrap_or("").split(',').filter_map(crate::named);
    ids.map(Id::from).collect()
}

/// What an operation summons, for one that does: the first thing its
/// tooltip names, past the likes of smoke marking where it lands.
fn summon(a: &defs::AbilityDefinition) -> Option<Summon> {
    let e = a
        .effect
        .iter()
        .find(|e| is(&e.kind, "SUMMON") && !e.hide_from_tooltip)?;
    let flag = |f: fn(&defs::AbilityFlags) -> bool| a.flags.iter().any(f);
    let unique = flag(|f| f.summon_unique).then(|| e.unique_target_flags.clone());
    Some(Summon {
        template: e.object_ids.clone()?,
        main_land: flag(|f| f.primary_island_only),
        one_at_a_time: !flag(|f| f.summon_multiple),
        unique: unique.flatten(),
    })
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

/// Whether each of `states` that says lets a regiment resupply, the first
/// for each id.
fn resupplying(states: impl Iterator<Item = defs::ActorState>) -> HashMap<Id, bool> {
    let mut out = HashMap::new();
    for state in states {
        if let (Some(id), Some(resupply)) = (state.ids, state.resupply) {
            out.entry(id).or_insert(resupply);
        }
    }
    out
}

/// The properties a template's abilities give what they land on.
pub(crate) fn enchantments(t: &defs::Thing) -> impl Iterator<Item = &defs::Property> {
    (t.abilities())
        .filter_map(|a| a.ability_definition.as_ref())
        .flat_map(|d| &d.effect)
        .filter_map(|e| e.enchantment_property.as_ref())
}

/// The properties a template's things hold of their own: the shared ones
/// it names, then its intrinsic ones.
pub(crate) fn held<'a>(
    t: &'a defs::Thing,
    shared: &'a HashMap<Id, defs::Property>,
) -> impl Iterator<Item = &'a defs::Property> {
    t.properties_component.iter().flat_map(move |c| {
        let refs = c.property.iter();
        let refs = refs.filter_map(move |r| shared.get(r.property_ids.as_ref()?));
        refs.chain(&c.intrinsic_property)
    })
}

impl GameData {
    pub fn supply_rules(&self) -> SupplyRules {
        let default = SupplyRules::default();
        let of = |get: fn(&defs::SVars) -> Option<f32>, or: f32| self.svar(get).unwrap_or(or);
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

    /// Whether a regiment may resupply while doing each activity, by its
    /// actor state id, as the first of the state files to say so has it.
    pub fn resupplying(&self) -> HashMap<Id, bool> {
        resupplying(self.actor_states().iter().cloned())
    }

    /// What each property does to supply, for those that do anything to
    /// it, by its id: the shared ones, technologies among them, and those
    /// abilities give.
    pub fn supply_effects(&self) -> HashMap<Id, SupplyEffect> {
        self.property_effects(effect)
    }

    /// Each property's enchantment type, for those that have one, by its
    /// id.
    pub fn enchantment_types(&self) -> HashMap<Id, Id> {
        self.property_effects(|p| p.enchantment_type.clone())
    }

    /// The properties a thing needs for each property to be on it, for
    /// those that need any, by its id.
    pub fn property_requirements(&self) -> HashMap<Id, Vec<Id>> {
        let mut all = self.property_effects(|p| Some(listed(p.required_properties.as_deref())));
        all.retain(|_, r| !r.is_empty());
        all
    }

    /// The properties a regiment keeps from its members when it holds
    /// them.
    pub fn kept_from_members(&self) -> HashSet<Id> {
        let kept = self.property_effects(|p| (!p.pass_down_members).then_some(()));
        kept.into_keys().collect()
    }

    /// What `effect` makes of each property, for those it makes anything
    /// of, by its id: the shared ones, technologies among them, and those
    /// abilities give.
    pub(crate) fn property_effects<E>(
        &self,
        effect: impl Fn(&defs::Property) -> Option<E>,
    ) -> HashMap<Id, E> {
        let definitions = self.definitions();
        let shared = definitions.properties.values();
        let given = definitions.given_properties().map(|(_, p)| p);
        // The first of those given more than once, in the order they load,
        // so every copy of a match reads the same.
        let mut out = HashMap::new();
        for p in shared.chain(given) {
            if let (Some(id), Some(effect)) = (p.ids.clone(), effect(p)) {
                out.entry(id).or_insert(effect);
            }
        }
        out
    }

    /// As [`GameData::property_effects`], with those experience levels give
    /// on top.
    pub(crate) fn effects_with_levels<E>(
        &self,
        effect: impl Fn(&defs::Property) -> Option<E>,
    ) -> HashMap<Id, E> {
        let mut out = self.property_effects(&effect);
        for (id, p) in self.level_properties() {
            if let Some(e) = effect(&p) {
                out.insert(id, e);
            }
        }
        out
    }

    /// Each template's [`Kind`], for those with properties, by template id.
    pub fn kinds(&self) -> HashMap<Id, Kind> {
        let definitions = self.definitions();
        self.by_template(|t| {
            let c = t.properties_component.as_ref()?;
            let properties: Vec<Id> =
                (c.property.iter().filter_map(|r| r.property_ids.clone())).collect();
            let flags = (held(t, &definitions.properties))
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
    pub fn decays(&self) -> HashMap<Id, f32> {
        let definitions = self.definitions();
        self.by_template(|t| {
            let periodic = |m: &&defs::Modifier| is(&m.kind, "PERIODIC_DAMAGE");
            let modifiers = held(t, &definitions.properties).flat_map(|p| &p.modifier);
            let rate: f32 = modifiers.filter(periodic).filter_map(|m| m.amount).sum();
            (rate > 0.0).then_some(rate)
        })
    }

    /// The special operations the match carries out, by id.
    pub fn operations(&self) -> HashMap<Id, Operation> {
        let definitions = self.definitions();
        let supplies = self.supplies();
        let flights = self.flights();
        let lookouts = self.lookouts();
        let mut out = HashMap::new();
        for (id, a) in &definitions.abilities {
            if let Some(s) = strike(a, &flights, &lookouts) {
                out.insert(id.clone(), Operation::Strike(Box::new(s)));
                continue;
            }
            let objects = a.effect.iter().filter_map(|e| e.object_ids.as_ref());
            let drops = objects.clone().find(|&o| supplies.contains_key(o)).cloned();
            // What it puts on the map is what it's for, whatever property it
            // gives besides.
            let operation = if let Some(seconds) = reveals(a, &definitions.properties) {
                Operation::Reveal { seconds }
            } else if let Some(template) = drops {
                Operation::Drop(template)
            } else if let Some(summon) = summon(a) {
                Operation::Summon(summon)
            } else if let Some(e) = enchantment(a, &flights) {
                Operation::Enchant(e)
            } else {
                continue;
            };
            out.insert(id.clone(), operation);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition(text: &str) -> defs::AbilityDefinition {
        let parsed = tgi::parse(text);
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("AbilityDefinition").next().unwrap();
        templates.deserialize(block).unwrap()
    }

    #[test]
    fn activities_say_whether_regiments_resupply_doing_them_the_first_saying_so() {
        let parsed = tgi::parse(
            "[ActorState]\n{\n\tIDS = mind_moving\n\tresupply = true\n}\n\
             [ActorState]\n{\n\tIDS = mind_bombard\n\tresupply = false\n}\n\
             [ActorState]\n{\n\tIDS = mind_bombard\n\tresupply = true\n}\n\
             [ActorState]\n{\n\tIDS = under_siege\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let states = (parsed.root.blocks_of("ActorState"))
            .map(|b| templates.deserialize::<defs::ActorState>(b).unwrap());
        let found = resupplying(states);
        assert_eq!(found.len(), 2);
        assert!(found[&Id::from("mind_moving")]);
        assert!(!found[&Id::from("mind_bombard")]);
    }

    #[test]
    fn summons_say_what_they_bring_and_what_holds_them_back() {
        let reorganization = definition(
            "[AbilityDefinition]\n{\n\tIDS = reorganization\n\
             \t[Flags]\n\tsummon_unique = true\n\tprimary_island_only = true\n\
             \t[Effect]\n\t{\n\t\ttype = SUMMON\n\t\tobject_ids = truck_corps\n\
             \t\tunique_target_flags = provides_corps\n\t}\n}\n",
        );
        let summon = |template: &str, main_land, one_at_a_time, unique: Option<&str>| Summon {
            template: Id::from(template),
            main_land,
            one_at_a_time,
            unique: unique.map(Id::from),
        };
        assert_eq!(
            super::summon(&reorganization),
            Some(summon("truck_corps", true, true, Some("provides_corps")))
        );
        // What's hidden from its tooltip isn't what it brings.
        let aid = definition(
            "[AbilityDefinition]\n{\n\tIDS = aid\n\
             \t[Effect]\n\t{\n\t\ttype = SUMMON\n\t\tobject_ids = smoke\n\t\thide_from_tooltip = true\n\t}\n\
             \t[Effect]\n\t{\n\t\ttype = SUMMON\n\t\tobject_ids = crate\n\t}\n}\n",
        );
        assert_eq!(
            super::summon(&aid).map(|s| s.template),
            Some(Id::from("crate"))
        );
        // A flag only counts with summon_unique.
        let maskirovka = definition(
            "[AbilityDefinition]\n{\n\tIDS = maskirovka\n\
             \t[Flags]\n\tsummon_multiple = true\n\
             \t[Effect]\n\t{\n\t\ttype = SUMMON\n\t\tobject_ids = decoys\n\
             \t\tunique_target_flags = provides_corps\n\t}\n}\n",
        );
        assert_eq!(
            super::summon(&maskirovka),
            Some(summon("decoys", false, false, None))
        );
    }
}
